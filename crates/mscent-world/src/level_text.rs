//! A level as text, one line per thing, so a level can be read, diffed and
//! edited without the editor or a rebuild.
//!
//! ```text
//! miniscent-level 1
//! # Comments and blank lines are ignored.
//! box subtract 0 1  -20 -20 -20  20 20 20         # op, order, material, min, max
//! box add 1 2  5 -3 -3  6 3 3
//! brush add 2 7  6 <plane> ... <plane>            # op, order, material, plane count
//! player 0 0 0  1.57                              # x y z yaw
//! light 0 0 8  1 0.9 0.7  16 1.2                  # x y z, r g b, radius, intensity
//! enemy 5 5 0  0  name guard  tag guards          # x y z kind
//! goal 8 8 0  1.5                                 # x y z radius
//! world 0 0 0  4 26                               # x y z, fog start, fog end
//! marker 3 0 0  0  name trap_hinge                # x y z yaw
//! zone 0 10 0  2 2 2  name hall  tag alarm        # x y z, half sizes
//! vista 0 0 0  0 0 0  0 0 0.0625  0 0 0.1  0 0  0 1e5  0x8090a0  # see below
//! mover vault_door  tag doors  brushes 1 2        # brush lines, counted from 0
//! ```
//!
//! A `vista` line is a vista layer: position, centre, slot, layer, parallax, spin
//! (axis times radians per second), pan, fog start, fog end, fog colour as decimal
//! or `0x` hex. Parallax may be written `x16` for 1/16, the scale of a room that
//! is authored 16 times smaller than it looks (a number of at least 1; it is
//! stored, and dumped, as the parallax). A `mountains` line builds a ridged-noise
//! range as a voxel volume (see `level_text/mountains.rs`), and is not dumped. A face's `material` of 65520 to 65527 is viewport slot 0 to 7.
//!
//! A plane in a `brush` line is `nx ny nz dist` then its texture mapping:
//! `material ux uy uz vx vy vz ushift vshift uscale vscale rotation lock`.
//! Dumping writes `box` for any brush a `box` line rebuilds exactly, and numbers
//! as the shortest text that reads back to the same bits.
//!
//! The text is untrusted like any level file. Everything read goes through the
//! binary format's own reader before it is returned, so both accept the same
//! levels.

use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::format::{self, MAX_BRUSHES, MAX_ENTITIES, MAX_FILE_BYTES};
use crate::labels::{EntityLabel, MoverGroup};
use crate::texinfo::TexInfo;
use mscent_math::{DPlane, Vec3, Vec3d};
use std::fmt::Write as _;

mod mountains;
mod words;

use words::{Res, Words, clip};

const HEADER: &str = "miniscent-level 1";

/// Why text is not a level, and on which line. Line 0 means the level as a
/// whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextError {
    pub line: usize,
    pub msg: String,
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

/// Writes `level` as text. [`from_text`] reads it back to an equal level.
pub fn to_text(level: &Level) -> String {
    let mut out = format!("{HEADER}\n");
    for b in &level.brushes {
        out += &brush_line(b);
    }
    for (i, e) in level.entities.iter().enumerate() {
        out += &entity_text(e);
        let label = level.labels.iter().find(|l| l.entity as usize == i);
        if let Some(label) = label {
            if let Some(name) = &label.name {
                let _ = write!(out, " name {name}");
            }
            label.tags.iter().for_each(|t| {
                let _ = write!(out, " tag {t}");
            });
        }
        out.push('\n');
    }
    for g in &level.mover_groups {
        let _ = write!(out, "mover {}", g.name);
        g.tags.iter().for_each(|t| {
            let _ = write!(out, " tag {t}");
        });
        out += " brushes";
        g.brushes.iter().for_each(|b| {
            let _ = write!(out, " {b}");
        });
        out.push('\n');
    }
    out
}

fn op_word(op: Op) -> &'static str {
    match op {
        Op::Add => "add",
        Op::Subtract => "subtract",
    }
}

fn v32(v: Vec3) -> String {
    format!("{} {} {}", v.x, v.y, v.z)
}

fn as_box(b: &Brush) -> Option<String> {
    let bounds = (b.planes().iter()).try_fold((Vec3d::ZERO, Vec3d::ZERO), |(lo, hi), p| {
        let n = p.normal;
        Some(match (n.x, n.y, n.z) {
            (1.0, 0.0, 0.0) => (lo, Vec3d::new(p.dist, hi.y, hi.z)),
            (-1.0, 0.0, 0.0) => (Vec3d::new(-p.dist, lo.y, lo.z), hi),
            (0.0, 1.0, 0.0) => (lo, Vec3d::new(hi.x, p.dist, hi.z)),
            (0.0, -1.0, 0.0) => (Vec3d::new(lo.x, -p.dist, lo.z), hi),
            (0.0, 0.0, 1.0) => (lo, Vec3d::new(hi.x, hi.y, p.dist)),
            (0.0, 0.0, -1.0) => (Vec3d::new(lo.x, lo.y, -p.dist), hi),
            _ => return None,
        })
    });
    let (lo, hi) = bounds?;
    let rebuilt = Brush::cuboid(lo, hi, b.op(), b.order()).ok()?;
    if rebuilt.with_material(b.material()) != *b {
        return None;
    }
    Some(format!(
        "box {} {} {} {} {} {} {} {} {}\n",
        op_word(b.op()),
        b.order(),
        b.material(),
        lo.x,
        lo.y,
        lo.z,
        hi.x,
        hi.y,
        hi.z
    ))
}

fn brush_line(b: &Brush) -> String {
    if let Some(line) = as_box(b) {
        return line;
    }
    let mut out = format!(
        "brush {} {} {} {}",
        op_word(b.op()),
        b.order(),
        b.material(),
        b.planes().len()
    );
    for (p, t) in b.planes().iter().zip(b.texinfos()) {
        let n = p.normal;
        let _ = write!(
            out,
            " {} {} {} {} {} {} {} {} {} {} {} {} {}",
            n.x,
            n.y,
            n.z,
            p.dist,
            t.material,
            v32(t.u_axis),
            v32(t.v_axis),
            t.u_shift,
            t.v_shift,
            t.u_scale,
            t.v_scale,
            t.rotation_deg,
            u8::from(t.lock_to_world)
        );
    }
    out.push('\n');
    out
}

fn entity_text(e: &Entity) -> String {
    match *e {
        Entity::PlayerStart { pos, yaw } => format!("player {} {yaw}", v32(pos)),
        Entity::Light {
            pos,
            color,
            radius,
            intensity,
        } => format!("light {} {} {radius} {intensity}", v32(pos), v32(color)),
        Entity::Enemy { pos, kind } => format!("enemy {} {kind}", v32(pos)),
        Entity::Goal { pos, radius } => format!("goal {} {radius}", v32(pos)),
        Entity::World {
            pos,
            fog_start,
            fog_end,
        } => format!("world {} {fog_start} {fog_end}", v32(pos)),
        Entity::Marker { pos, yaw } => format!("marker {} {yaw}", v32(pos)),
        Entity::Zone { pos, half } => format!("zone {} {}", v32(pos), v32(half)),
        Entity::VistaLayer {
            pos,
            centre,
            slot,
            layer,
            parallax,
            spin,
            pan,
            fog_start,
            fog_end,
            fog_color,
        } => format!(
            "vista {} {} {slot} {layer} {parallax} {} {} {} {fog_start} {fog_end} {fog_color:#08x}",
            v32(pos),
            v32(centre),
            v32(spin),
            pan[0],
            pan[1]
        ),
    }
}

fn read_op(w: &mut Words) -> Res<Op> {
    match w.word("add or subtract")? {
        "add" => Ok(Op::Add),
        "subtract" => Ok(Op::Subtract),
        other => Err(format!("expected add or subtract, not '{}'", clip(other))),
    }
}

fn read_box(w: &mut Words) -> Res<Brush> {
    let (op, order) = (read_op(w)?, w.parse("the order")?);
    let material: u16 = w.parse("the material")?;
    let (lo, hi) = (w.v3d("the min corner")?, w.v3d("the max corner")?);
    let brush = Brush::cuboid(lo, hi, op, order).map_err(|e| format!("bad box: {e:?}"))?;
    Ok(brush.with_material(material))
}

fn read_plane(w: &mut Words) -> Res<(DPlane, TexInfo)> {
    let plane = DPlane::new(w.v3d("a plane normal")?, w.f64("a plane distance")?);
    let info = TexInfo {
        material: w.parse("a face material")?,
        u_axis: w.v3("a texture axis")?,
        v_axis: w.v3("a texture axis")?,
        u_shift: w.f32("a texture shift")?,
        v_shift: w.f32("a texture shift")?,
        u_scale: w.f32("a texture scale")?,
        v_scale: w.f32("a texture scale")?,
        rotation_deg: w.f32("a texture rotation")?,
        lock_to_world: match w.word("0 or 1")? {
            "0" => false,
            "1" => true,
            other => return Err(format!("expected 0 or 1, not '{}'", clip(other))),
        },
    };
    Ok((plane, info))
}

fn read_brush(w: &mut Words) -> Res<Brush> {
    let (op, order) = (read_op(w)?, w.parse("the order")?);
    let _material: u16 = w.parse("the material")?;
    let count: usize = w.parse("the plane count")?;
    if count > crate::brush::MAX_PLANES {
        return Err(format!("{count} planes is too many"));
    }
    let mut planes = Vec::with_capacity(count);
    let mut infos = Vec::with_capacity(count);
    for _ in 0..count {
        let (p, t) = read_plane(w)?;
        planes.push(p);
        infos.push(t);
    }
    Brush::new_textured(&planes, &infos, op, order).map_err(|e| format!("bad brush: {e:?}"))
}

fn read_entity(kind: &str, w: &mut Words) -> Res<Option<Entity>> {
    let pos = |w: &mut Words| w.v3("a position");
    Ok(Some(match kind {
        "player" => Entity::PlayerStart {
            pos: pos(w)?,
            yaw: w.f32("the yaw")?,
        },
        "light" => Entity::Light {
            pos: pos(w)?,
            color: w.v3("a colour")?,
            radius: w.f32("the radius")?,
            intensity: w.f32("the intensity")?,
        },
        "enemy" => Entity::Enemy {
            pos: pos(w)?,
            kind: w.parse("the kind")?,
        },
        "goal" => Entity::Goal {
            pos: pos(w)?,
            radius: w.f32("the radius")?,
        },
        "world" => Entity::World {
            pos: pos(w)?,
            fog_start: w.f32("the fog start")?,
            fog_end: w.f32("the fog end")?,
        },
        "marker" => Entity::Marker {
            pos: pos(w)?,
            yaw: w.f32("the yaw")?,
        },
        "zone" => Entity::Zone {
            pos: pos(w)?,
            half: w.v3("the half sizes")?,
        },
        "vista" => Entity::VistaLayer {
            pos: pos(w)?,
            centre: w.v3("the centre")?,
            slot: w.parse("the slot")?,
            layer: w.parse("the layer")?,
            parallax: w.parallax("the parallax")?,
            spin: w.v3("the spin")?,
            pan: [w.f32("the pan")?, w.f32("the pan")?],
            fog_start: w.f32("the fog start")?,
            fog_end: w.f32("the fog end")?,
            fog_color: w.colour("the fog colour")?,
        },
        _ => return Ok(None),
    }))
}

/// `name x` and `tag y` after an entity.
fn read_label(index: usize, w: &mut Words) -> Res<Option<EntityLabel>> {
    let (mut name, mut tags) = (None, Vec::new());
    while let Some(word) = w.rest.next() {
        match word {
            "name" => name = Some(w.word("a name")?.to_string()),
            "tag" => tags.push(w.word("a tag")?.to_string()),
            other => return Err(format!("expected name or tag, not '{}'", clip(other))),
        }
    }
    if name.is_none() && tags.is_empty() {
        return Ok(None);
    }
    let entity = u32::try_from(index).map_err(|_| "too many entities".to_string())?;
    Ok(Some(EntityLabel { entity, name, tags }))
}

fn read_mover(w: &mut Words) -> Res<MoverGroup> {
    let name = w.word("the mover's name")?.to_string();
    let mut tags = Vec::new();
    loop {
        match w.word("tag or brushes")? {
            "tag" => tags.push(w.word("a tag")?.to_string()),
            "brushes" => break,
            other => return Err(format!("expected tag or brushes, not '{}'", clip(other))),
        }
    }
    let mut brushes = Vec::new();
    for word in w.rest.by_ref() {
        let index = word
            .parse()
            .map_err(|_| format!("bad brush index '{}'", clip(word)))?;
        brushes.push(index);
        if brushes.len() > crate::labels::MAX_GROUP_BRUSHES {
            return Err("too many brushes in one mover".to_string());
        }
    }
    Ok(MoverGroup {
        name,
        tags,
        brushes,
    })
}

fn read_line(level: &mut Level, line: &str) -> Res<()> {
    let code = line.split('#').next().unwrap_or("");
    let mut w = Words {
        rest: code.split_whitespace(),
    };
    let Some(kind) = w.rest.next() else {
        return Ok(());
    };
    match kind {
        "box" | "brush" => {
            if level.brushes.len() >= MAX_BRUSHES {
                return Err("too many brushes".to_string());
            }
            let brush = if kind == "box" {
                read_box(&mut w)?
            } else {
                read_brush(&mut w)?
            };
            level.brushes.push(brush);
        }
        "mover" => level.mover_groups.push(read_mover(&mut w)?),
        "mountains" => {
            let volume = mountains::read(&mut w, level)?;
            level.volumes.push(volume);
        }
        _ => {
            if level.entities.len() >= MAX_ENTITIES {
                return Err("too many entities".to_string());
            }
            let Some(entity) = read_entity(kind, &mut w)? else {
                return Err(format!("unknown line type '{}'", clip(kind)));
            };
            if !entity.is_valid() {
                return Err("a value is out of range".to_string());
            }
            if let Some(label) = read_label(level.entities.len(), &mut w)? {
                level.labels.push(label);
            }
            level.entities.push(entity);
        }
    }
    if !matches!(kind, "mover" | "box" | "brush" | "mountains") {
        return Ok(());
    }
    match w.rest.next() {
        Some(extra) => Err(format!(
            "unexpected '{}' at the end of the line",
            clip(extra)
        )),
        None => Ok(()),
    }
}

/// Reads a level. Fails on the first problem, with its line.
pub fn from_text(text: &str) -> Result<Level, TextError> {
    let whole = |msg: String| TextError { line: 0, msg };
    if text.len() > MAX_FILE_BYTES {
        return Err(whole("the file is too large".to_string()));
    }
    let mut lines = text.lines().enumerate();
    let first = lines.next().map(|(_, l)| l.trim());
    if first != Some(HEADER) {
        return Err(TextError {
            line: 1,
            msg: format!("the first line must be '{HEADER}'"),
        });
    }
    let mut level = Level::default();
    for (i, line) in lines {
        read_line(&mut level, line).map_err(|msg| TextError { line: i + 1, msg })?;
    }
    // The binary reader is the one judge of what a level may be.
    format::read(&format::write(&level)).map_err(|e| whole(format!("{e:?}")))
}
