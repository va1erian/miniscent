//! Reading a blueprint: versioned text, diffable, and read as untrusted input.
//!
//! ```text
//! miniscent-blueprint 1
//! name mantis-07
//! class alien
//! plan mantis 7
//! scale 0.25
//! skin hull chitin
//! skin eye glass glow
//! part body cell=1
//! add spheroid 6 0 0  1 0 0  2.8 2  hull
//! carve sphere 7.5 1.4 1.5  0.6
//! part blade pivot=8,4.5,1 axis=0,0,1 motion=swing:0.6:2
//! solid plate 0.5  7 4 1  10 4 3  18 4 0  hull detail
//! hit 6 0 0 2.2
//! ```
//!
//! Skins come before the parts that use them, as a role must exist to be named.
//! Everything is checked here, including that each shape makes a solid, since the
//! check is cheap and what comes after can then only fail on what the build finds.

use mscent_math::Vec3d;

use super::cost::part_box;
use super::read_shape::{hard, number, numbers, positive, soft};
use super::{
    Blueprint, Class, DEFAULT_SCALE, Hit, MAX_CUT_BRUSHES, MAX_HITS, MAX_MODEL_CHUNKS, MAX_NAME,
    MAX_PARTS, MAX_PIECES, MAX_ROLES, MAX_SCALE, ModelError, Motion, Part, Piece, Skin,
};
use crate::voxel::CellSize;

pub(super) const HEADER: &str = "miniscent-blueprint 1";

/// Largest blueprint file, in bytes.
pub const MAX_FILE: usize = 64 * 1024;
/// Largest rate of a part's motion, in cycles a second.
const MAX_RATE: f64 = 64.0;

type Parsed<T> = Result<T, String>;

/// Reads a blueprint. Never panics, whatever the bytes.
pub fn read(bytes: &[u8]) -> Result<Blueprint, ModelError> {
    let whole = |why: &str| ModelError::Parse {
        line: 0,
        why: why.to_string(),
    };
    if bytes.len() > MAX_FILE {
        return Err(whole("the file is too large"));
    }
    let text = core::str::from_utf8(bytes).map_err(|_| whole("the file is not text"))?;
    let mut lines = text.lines().enumerate();
    match lines.next() {
        Some((_, first)) if first.trim_end() == HEADER => {}
        _ => return Err(whole("the first line is not the blueprint header")),
    }
    let mut draft = Draft::default();
    for (i, line) in lines {
        let content = line.split('#').next().unwrap_or("");
        let words: Vec<&str> = content.split_whitespace().collect();
        if let Some((&word, rest)) = words.split_first() {
            draft
                .line(word, rest)
                .map_err(|why| ModelError::Parse { line: i + 1, why })?;
        }
    }
    draft.finish().map_err(|why| whole(&why))
}

/// What the lines so far have said.
#[derive(Default)]
struct Draft {
    name: Option<String>,
    class: Option<Class>,
    plan: Option<(String, u64)>,
    scale: Option<f64>,
    skins: Vec<Skin>,
    parts: Vec<Part>,
    hits: Vec<Hit>,
    pieces: usize,
}

impl Draft {
    fn line(&mut self, word: &str, rest: &[&str]) -> Parsed<()> {
        match word {
            "name" => self.name = Some(one_name(rest)?),
            "class" => self.class = Some(class(rest)?),
            "plan" => self.plan = Some(plan(rest)?),
            "scale" => self.scale = Some(scale(rest)?),
            "skin" => self.skin(rest)?,
            "part" => self.part(rest)?,
            "hit" => self.hit(rest)?,
            "solid" | "cut" | "add" | "carve" | "smooth" | "paint" => self.piece(word, rest)?,
            _ => return Err(format!("unknown line '{word}'")),
        }
        Ok(())
    }

    fn skin(&mut self, rest: &[&str]) -> Parsed<()> {
        if self.skins.len() >= MAX_ROLES {
            return Err("too many skins".into());
        }
        let (role, material, glow) = match *rest {
            [role, material] => (role, material, false),
            [role, material, "glow"] => (role, material, true),
            _ => return Err("a skin is a role, a material and perhaps 'glow'".into()),
        };
        let role = name(role)?;
        if self.skins.iter().any(|s| s.role == role) {
            return Err(format!("role '{role}' is given twice"));
        }
        self.skins.push(Skin {
            role,
            material: name(material)?,
            glow,
        });
        Ok(())
    }

    fn part(&mut self, rest: &[&str]) -> Parsed<()> {
        if self.parts.len() >= MAX_PARTS {
            return Err("too many parts".into());
        }
        let (first, options) = rest.split_first().ok_or("a part needs a name")?;
        let part_name = name(first)?;
        if self.parts.iter().any(|p| p.name == part_name) {
            return Err(format!("part '{part_name}' is given twice"));
        }
        let mut part = Part {
            name: part_name,
            cell: CellSize::Half,
            pivot: Vec3d::ZERO,
            axis: Vec3d::new(0.0, 0.0, 1.0),
            motion: Motion::Still,
            pieces: Vec::new(),
        };
        for option in options {
            let (key, value) = option.split_once('=').ok_or("an option is key=value")?;
            match key {
                "cell" => part.cell = cell(value)?,
                "pivot" => part.pivot = vec3_of(value)?,
                "axis" => part.axis = unit(vec3_of(value)?)?,
                "motion" => part.motion = motion(value)?,
                _ => return Err(format!("unknown part option '{key}'")),
            }
        }
        self.parts.push(part);
        Ok(())
    }

    fn hit(&mut self, rest: &[&str]) -> Parsed<()> {
        if self.hits.len() >= MAX_HITS {
            return Err("too many hit spheres".into());
        }
        let n = numbers(rest)?;
        let &[x, y, z, radius] = n.as_slice() else {
            return Err("a hit sphere is a centre and a radius".into());
        };
        self.hits.push(Hit {
            centre: Vec3d::new(x, y, z),
            radius: positive(radius)?,
        });
        Ok(())
    }

    fn piece(&mut self, word: &str, rest: &[&str]) -> Parsed<()> {
        if self.pieces >= MAX_PIECES {
            return Err("too many pieces".into());
        }
        let skins = &self.skins;
        let part = self
            .parts
            .last_mut()
            .ok_or("a piece needs a part above it")?;
        let piece = match word {
            "solid" => {
                let (rest, detail) = match rest.split_last() {
                    Some((&"detail", head)) => (head, true),
                    _ => (rest, false),
                };
                let (rest, role) = with_role(rest, skins)?;
                let shape = hard(rest)?;
                Piece::Solid {
                    shape,
                    role,
                    detail,
                }
            }
            "cut" => {
                // Walls the file gives no role for look like the first skin.
                let named = rest.len() > 1 && rest.last().is_some_and(|w| number(w).is_err());
                let (rest, role) = if named {
                    with_role(rest, skins)?
                } else {
                    (rest, first_role(skins)?)
                };
                Piece::Cut {
                    shape: hard(rest)?,
                    role,
                }
            }
            "add" => {
                let (rest, role) = with_role(rest, skins)?;
                Piece::Add {
                    shape: soft(rest)?,
                    role,
                }
            }
            "paint" => {
                let (rest, role) = with_role(rest, skins)?;
                Piece::Paint {
                    shape: soft(rest)?,
                    role,
                }
            }
            "carve" => Piece::Carve { shape: soft(rest)? },
            _ => Piece::Smooth { shape: soft(rest)? },
        };
        part.pieces.push(piece);
        self.pieces += 1;
        Ok(())
    }

    fn finish(self) -> Parsed<Blueprint> {
        if self.parts.is_empty() {
            return Err("a blueprint needs a part".into());
        }
        let mut chunks = 0usize;
        for part in &self.parts {
            check_part(part)?;
            if let Some(b) = part_box(part) {
                chunks = chunks.saturating_add(b.chunk_count());
            }
        }
        if chunks > MAX_MODEL_CHUNKS {
            return Err(format!(
                "the voxel parts cover {chunks} chunks, and the limit is {MAX_MODEL_CHUNKS}"
            ));
        }
        Ok(Blueprint {
            name: self.name.ok_or("a blueprint needs a name")?,
            class: self.class.ok_or("a blueprint needs a class")?,
            plan: self.plan,
            scale: self.scale.unwrap_or(DEFAULT_SCALE),
            skins: self.skins,
            parts: self.parts,
            hits: self.hits,
        })
    }
}

/// What can only be said of a part once all its pieces are in.
fn check_part(part: &Part) -> Parsed<()> {
    if part.pieces.is_empty() {
        return Err(format!("part '{}' has no pieces", part.name));
    }
    let count = |f: fn(&Piece) -> bool| part.pieces.iter().filter(|p| f(p)).count();
    let cuts = count(|p| matches!(p, Piece::Cut { .. }));
    let solids = count(|p| matches!(p, Piece::Solid { .. }));
    if cuts > 0 && solids + cuts > MAX_CUT_BRUSHES {
        return Err(format!(
            "part '{}' cuts {} brushes, and the limit is {MAX_CUT_BRUSHES}",
            part.name,
            solids + cuts
        ));
    }
    if part_box_too_long(part) {
        return Err(format!("part '{}' is too long for its cell", part.name));
    }
    Ok(())
}

/// Whether a part's box needs more chunks along an axis than a volume can hold.
fn part_box_too_long(part: &Part) -> bool {
    part_box(part).is_some_and(|b| b.volume().is_err())
}

fn role_index(word: &str, skins: &[Skin]) -> Parsed<u8> {
    skins
        .iter()
        .position(|s| s.role == word)
        .and_then(|i| u8::try_from(i).ok())
        .ok_or_else(|| format!("no skin for role '{word}'"))
}

fn first_role(skins: &[Skin]) -> Parsed<u8> {
    if skins.is_empty() {
        return Err("a cut with no role needs a skin above it".into());
    }
    Ok(0)
}

fn with_role<'a, 'b>(rest: &'a [&'b str], skins: &[Skin]) -> Parsed<(&'a [&'b str], u8)> {
    let (last, head) = rest.split_last().ok_or("this piece needs a role")?;
    Ok((head, role_index(last, skins)?))
}

fn valid_name(s: &str) -> bool {
    let chars = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    !s.is_empty() && s.len() <= MAX_NAME && s.chars().all(chars)
}

fn name(s: &str) -> Parsed<String> {
    if valid_name(s) {
        Ok(s.to_string())
    } else {
        Err(format!(
            "'{s}' is not a name: up to {MAX_NAME} of a-z, 0-9 and -"
        ))
    }
}

fn one_name(rest: &[&str]) -> Parsed<String> {
    match *rest {
        [s] => name(s),
        _ => Err("this line takes one name".into()),
    }
}

fn class(rest: &[&str]) -> Parsed<Class> {
    match *rest {
        ["drone"] => Ok(Class::Drone),
        ["ship"] => Ok(Class::Ship),
        ["alien"] => Ok(Class::Alien),
        _ => Err("a class is drone, ship or alien".into()),
    }
}

fn plan(rest: &[&str]) -> Parsed<(String, u64)> {
    let &[plan, seed] = rest else {
        return Err("a plan is a name and a seed".into());
    };
    let seed = seed.parse().map_err(|_| "a seed is a whole number")?;
    Ok((name(plan)?, seed))
}

fn scale(rest: &[&str]) -> Parsed<f64> {
    let n = numbers(rest)?;
    match *n.as_slice() {
        [s] if s > 0.0 && s <= MAX_SCALE => Ok(s),
        _ => Err(format!("a scale is a number above 0, up to {MAX_SCALE}")),
    }
}

fn cell(s: &str) -> Parsed<CellSize> {
    match s {
        "0.125" => Ok(CellSize::Eighth),
        "0.25" => Ok(CellSize::Quarter),
        "0.5" => Ok(CellSize::Half),
        "1" => Ok(CellSize::One),
        "2" => Ok(CellSize::Two),
        "4" => Ok(CellSize::Four),
        _ => Err("a cell is 0.125, 0.25, 0.5, 1, 2 or 4".into()),
    }
}

/// `spin:rate`, `swing:angle:rate`, `sway:reach:rate` or `still`.
fn motion(s: &str) -> Parsed<Motion> {
    let mut words = s.split(':');
    let kind = words.next().unwrap_or("");
    let values = words.map(number).collect::<Parsed<Vec<f64>>>()?;
    let rate = |r: f64| {
        if r.abs() <= MAX_RATE {
            Ok(r)
        } else {
            Err(format!("a rate is up to {MAX_RATE}"))
        }
    };
    match (kind, values.as_slice()) {
        ("still", []) => Ok(Motion::Still),
        ("spin", &[r]) => Ok(Motion::Spin { rate: rate(r)? }),
        ("swing", &[angle, r]) => Ok(Motion::Swing {
            angle,
            rate: rate(r)?,
        }),
        ("sway", &[reach, r]) => Ok(Motion::Sway {
            reach,
            rate: rate(r)?,
        }),
        _ => Err(format!("unknown motion '{s}'")),
    }
}

fn vec3_of(s: &str) -> Parsed<Vec3d> {
    let n = s.split(',').map(number).collect::<Parsed<Vec<f64>>>()?;
    let &[x, y, z] = n.as_slice() else {
        return Err("a point is x,y,z".into());
    };
    Ok(Vec3d::new(x, y, z))
}

fn unit(v: Vec3d) -> Parsed<Vec3d> {
    v.normalized()
        .ok_or_else(|| "an axis needs a length".into())
}
