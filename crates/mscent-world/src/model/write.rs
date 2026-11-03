//! Writing a blueprint, so that [`read`](super::read) gives back the same one.
//! A number prints as the shortest text that reads back as itself.

use core::fmt::Write;

use mscent_math::Vec3d;

use super::read::HEADER;
use super::{Blueprint, Class, Hard, Motion, Part, Piece, Soft};
use crate::voxel::CellSize;

/// The text of a blueprint.
pub fn write(b: &Blueprint) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{HEADER}");
    let _ = writeln!(out, "name {}", b.name);
    let _ = writeln!(out, "class {}", class_word(b.class));
    if let Some((plan, seed)) = &b.plan {
        let _ = writeln!(out, "plan {plan} {seed}");
    }
    let _ = writeln!(out, "scale {}", b.scale);
    for s in &b.skins {
        let glow = if s.glow { " glow" } else { "" };
        let _ = writeln!(out, "skin {} {}{glow}", s.role, s.material);
    }
    let roles: Vec<&str> = b.skins.iter().map(|s| s.role.as_str()).collect();
    for part in &b.parts {
        write_part(&mut out, part, &roles);
    }
    for h in &b.hits {
        let _ = writeln!(out, "hit {} {}", spaced(h.centre), h.radius);
    }
    out
}

fn class_word(c: Class) -> &'static str {
    match c {
        Class::Drone => "drone",
        Class::Ship => "ship",
        Class::Alien => "alien",
    }
}

fn cell_word(c: CellSize) -> &'static str {
    match c {
        CellSize::Eighth => "0.125",
        CellSize::Quarter => "0.25",
        CellSize::Half => "0.5",
        CellSize::One => "1",
        CellSize::Two => "2",
        CellSize::Four => "4",
    }
}

fn motion_word(m: Motion) -> String {
    match m {
        Motion::Still => "still".into(),
        Motion::Spin { rate } => format!("spin:{rate}"),
        Motion::Swing { angle, rate } => format!("swing:{angle}:{rate}"),
        Motion::Sway { reach, rate } => format!("sway:{reach}:{rate}"),
    }
}

fn spaced(p: Vec3d) -> String {
    format!("{} {} {}", p.x, p.y, p.z)
}

fn commas(p: Vec3d) -> String {
    format!("{},{},{}", p.x, p.y, p.z)
}

fn write_part(out: &mut String, part: &Part, roles: &[&str]) {
    let _ = writeln!(
        out,
        "part {} cell={} pivot={} axis={} motion={}",
        part.name,
        cell_word(part.cell),
        commas(part.pivot),
        commas(part.axis),
        motion_word(part.motion),
    );
    let role = |r: u8| roles.get(usize::from(r)).copied().unwrap_or("");
    for piece in &part.pieces {
        let _ = match *piece {
            Piece::Solid {
                shape,
                role: r,
                detail,
            } => {
                let detail = if detail { " detail" } else { "" };
                writeln!(out, "solid {} {}{detail}", hard_words(&shape), role(r))
            }
            Piece::Cut { shape, role: r } => {
                writeln!(out, "cut {} {}", hard_words(&shape), role(r))
            }
            Piece::Add { shape, role: r } => {
                writeln!(out, "add {} {}", soft_words(&shape), role(r))
            }
            Piece::Paint { shape, role: r } => {
                writeln!(out, "paint {} {}", soft_words(&shape), role(r))
            }
            Piece::Carve { shape } => writeln!(out, "carve {}", soft_words(&shape)),
            Piece::Smooth { shape } => writeln!(out, "smooth {}", soft_words(&shape)),
        };
    }
}

fn hard_words(shape: &Hard) -> String {
    match *shape {
        Hard::Cube { centre, half } => format!("cube {} {}", spaced(centre), spaced(half)),
        Hard::Ngon {
            a,
            b,
            ra,
            rb,
            sides,
        } => format!("ngon {} {} {ra} {rb} {sides}", spaced(a), spaced(b)),
        Hard::Plate {
            thick,
            count,
            ref points,
        } => {
            let mut s = format!("plate {thick}");
            for p in &points[..usize::from(count)] {
                let _ = write!(s, " {}", spaced(*p));
            }
            s
        }
    }
}

fn soft_words(shape: &Soft) -> String {
    match *shape {
        Soft::Sphere { centre, radius } => format!("sphere {} {radius}", spaced(centre)),
        Soft::RoundCone { a, b, ra, rb } => {
            format!("round_cone {} {} {ra} {rb}", spaced(a), spaced(b))
        }
        Soft::Cuboid { min, max } => format!("cuboid {} {}", spaced(min), spaced(max)),
        Soft::Spheroid {
            centre,
            axis,
            along,
            across,
        } => format!(
            "spheroid {} {} {along} {across}",
            spaced(centre),
            spaced(axis)
        ),
        Soft::HexPrism {
            centre,
            axis,
            side,
            apothem,
            half_depth,
        } => format!(
            "hex_prism {} {} {} {apothem} {half_depth}",
            spaced(centre),
            spaced(axis),
            spaced(side)
        ),
    }
}
