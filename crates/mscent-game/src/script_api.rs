//! The game functions scripts call: the one table, and those about finding
//! things and moving them. Spawning, groups and the player are in
//! `script_world.rs`.
//!
//! A function reports a bad request by returning an error, which becomes a
//! script error on the calling line. Handles come only from these functions,
//! so a script cannot forge one, but any value can arrive as any argument.

use crate::arena::Thing;
use crate::game::Game;
use crate::mover::{MoverError, WhenBlocked};
use crate::script::{Handler, MAX_HANDLERS, Trigger};
use crate::script_world;
use mscent_collide::Pose;
use mscent_math::Vec3;
use mscent_script::{Flow, Heap, Native, Value};

pub(crate) const MOVER: u8 = 1;
pub(crate) const ZONE: u8 = 2;
pub(crate) const MARKER: u8 = 3;
pub(crate) const ENEMY: u8 = 4;
pub(crate) const GROUP: u8 = 5;
pub(crate) const PLAYER: u8 = 6;
pub(crate) const KIND: u8 = 7;
pub(crate) const PROP: u8 = 8;
pub(crate) const PROP_KIND: u8 = 9;

pub(crate) type Res = Result<Flow, String>;

/// The largest turn one pose can hold: a rotation is stored as a quaternion,
/// which goes the short way round.
const MAX_TURN_DEGREES: f32 = 180.0;

/// Every game function, by the message that calls it. `--script-api` and the
/// load-time checks both come from this table.
pub fn natives() -> Vec<(&'static str, Native<Game>)> {
    let mut table: Vec<(&'static str, Native<Game>)> = vec![
        ("mover:", find_mover),
        ("zone:", find_zone),
        ("marker:", find_marker),
        ("enemy:", find_enemy),
        ("markers:", find_markers),
        ("pose:move:", pose_move),
        ("pose:turn:degrees:about:", pose_turn),
        ("moveTo:in:", move_to),
        ("poseNow", pose_now),
        ("moving", moving),
        ("whenBlocked:", when_blocked),
        ("whenBlocked:damage:", when_blocked_damage),
        ("onArrived:", |g, _, r, a| {
            handler(g, Trigger::Arrived(mover_of(r)? as u32), a, false)
        }),
        ("onEnter:", |g, _, r, a| {
            handler(g, Trigger::ZoneEnter(zone_of(r)?), a, false)
        }),
        ("onceEnter:", |g, _, r, a| {
            handler(g, Trigger::ZoneEnter(zone_of(r)?), a, true)
        }),
        ("onExit:", |g, _, r, a| {
            handler(g, Trigger::ZoneExit(zone_of(r)?), a, false)
        }),
        ("hasPlayer", has_player),
        ("x:y:z:", vector),
    ];
    table.extend_from_slice(script_world::NATIVES);
    table.extend_from_slice(crate::script_props::NATIVES);
    table
}

pub(crate) fn val(v: Value) -> Res {
    Ok(Flow::Val(v))
}

pub(crate) fn nil() -> Res {
    val(Value::Nil)
}

pub(crate) fn one(args: &[Value]) -> Result<Value, String> {
    args.first()
        .copied()
        .ok_or_else(|| "a value is missing".to_string())
}

pub(crate) fn string(h: &Heap, v: Value, what: &str) -> Result<String, String> {
    h.str(v)
        .map(str::to_string)
        .ok_or_else(|| format!("{what} must be a string, not {}", h.describe(v)))
}

pub(crate) fn number(h: &Heap, v: Value, what: &str) -> Result<f32, String> {
    match v {
        Value::Num(n) if n.is_finite() => Ok(n),
        _ => Err(format!("{what} must be a number, not {}", h.describe(v))),
    }
}

/// A point or direction: `x: y: z:`, or a marker's position.
pub(crate) fn point(g: &Game, h: &Heap, v: Value) -> Result<Vec3, String> {
    if let Value::Handle(MARKER, i) = v {
        let marker = g.arena.markers.get(i as usize);
        return marker
            .map(|m| m.pos)
            .ok_or_else(|| "no such marker".to_string());
    }
    match h.array(v) {
        Some(&[Value::Num(x), Value::Num(y), Value::Num(z)]) => Ok(Vec3::new(x, y, z)),
        _ => Err(format!(
            "wanted a marker or (x: y: z:), not {}",
            h.describe(v)
        )),
    }
}

fn vector(_: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[x, y, z] = args else {
        return Err("x:y:z: wants three numbers".into());
    };
    let nums = [
        Value::Num(number(h, x, "x:")?),
        Value::Num(number(h, y, "y:")?),
        Value::Num(number(h, z, "z:")?),
    ];
    h.new_array(nums.to_vec()).map(Flow::Val)
}

fn lookup(g: &Game, h: &Heap, args: &[Value], what: &str) -> Result<Thing, String> {
    let name = string(h, one(args)?, what)?;
    g.arena
        .names
        .by_name
        .get(&name)
        .copied()
        .ok_or_else(|| format!("the level has no {what} named '{name}'"))
}

fn find_mover(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    match lookup(g, h, args, "mover")? {
        Thing::Mover(i) => val(Value::Handle(MOVER, i as u32)),
        _ => Err("that name is not a mover".into()),
    }
}

fn find_zone(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    match lookup(g, h, args, "zone")? {
        Thing::Zone(i) => val(Value::Handle(ZONE, i as u32)),
        _ => Err("that name is not a zone".into()),
    }
}

fn find_marker(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    match lookup(g, h, args, "marker")? {
        Thing::Marker(i) => val(Value::Handle(MARKER, i as u32)),
        _ => Err("that name is not a marker".into()),
    }
}

fn find_enemy(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    match lookup(g, h, args, "enemy")? {
        Thing::Enemy(i) => val(Value::Handle(ENEMY, i as u32)),
        _ => Err("that name is not an enemy".into()),
    }
}

/// Markers with this tag, or whose names start with it, in name order. None at
/// all is an error: a typo here would otherwise spawn nothing and wait forever.
fn find_markers(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let prefix = string(h, one(args)?, "markers:")?;
    let found: Vec<Value> = g
        .arena
        .names
        .markers_for(&prefix)
        .into_iter()
        .map(|i| Value::Handle(MARKER, i as u32))
        .collect();
    if found.is_empty() {
        return Err(format!(
            "the level has no markers named or tagged '{prefix}'"
        ));
    }
    h.new_array(found).map(Flow::Val)
}

pub(crate) fn mover_of(v: Value) -> Result<usize, String> {
    match v {
        Value::Handle(MOVER, i) => Ok(i as usize),
        _ => Err("this message is for a mover".into()),
    }
}

fn zone_of(v: Value) -> Result<u32, String> {
    match v {
        Value::Handle(ZONE, i) => Ok(i),
        _ => Err("this message is for a zone".into()),
    }
}

fn mover_error(e: MoverError, name: &str) -> String {
    match e {
        MoverError::NoSuchMover => "no such mover".to_string(),
        MoverError::NoSuchPose => format!("this mover has no pose '{name}'"),
        MoverError::BadPoseName => format!("cannot define pose '{name}': too many, or 'closed'"),
        MoverError::BadPose => format!("pose '{name}' is out of range"),
        MoverError::BadTime => "the time must be between 0 and an hour".to_string(),
        MoverError::BadDamage => "the damage must not be negative".to_string(),
    }
}

fn pose_move(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let &[name, travel] = args else {
        return Err("pose:move: wants a name and a vector".into());
    };
    let (index, name) = (mover_of(r)?, string(h, name, "the pose")?);
    let pose = Pose::translated(point(g, h, travel)?.to_f64());
    g.define_pose(index, &name, pose)
        .map_err(|e| mover_error(e, &name))?;
    val(r)
}

fn pose_turn(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let &[name, axis, degrees, about] = args else {
        return Err("pose:turn:degrees:about: wants four values".into());
    };
    let (index, name) = (mover_of(r)?, string(h, name, "the pose")?);
    let degrees = number(h, degrees, "degrees:")?;
    if degrees.abs() > MAX_TURN_DEGREES {
        return Err(format!("a pose turns at most {MAX_TURN_DEGREES} degrees"));
    }
    let axis = point(g, h, axis)?;
    if axis.normalized().is_none() {
        return Err("turn: needs a direction, not zero".into());
    }
    let pivot = point(g, h, about)?.to_f64();
    let pose = Pose::turned(axis, degrees.to_radians(), pivot);
    g.define_pose(index, &name, pose)
        .map_err(|e| mover_error(e, &name))?;
    val(r)
}

fn move_to(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let &[pose, seconds] = args else {
        return Err("moveTo:in: wants a pose and a time".into());
    };
    let (index, pose) = (mover_of(r)?, string(h, pose, "the pose")?);
    let seconds = number(h, seconds, "in:")?;
    g.move_mover(index, &pose, seconds)
        .map_err(|e| mover_error(e, &pose))?;
    nil()
}

fn pose_now(g: &mut Game, h: &mut Heap, r: Value, _: &[Value]) -> Res {
    let mover = g.mover(mover_of(r)?).ok_or("no such mover")?;
    match mover.pose_name() {
        Some(name) => h.new_str(name.to_string()).map(Flow::Val),
        None => nil(),
    }
}

fn moving(g: &mut Game, _: &mut Heap, r: Value, _: &[Value]) -> Res {
    let mover = g.mover(mover_of(r)?).ok_or("no such mover")?;
    val(Value::Bool(mover.is_moving()))
}

fn when_blocked(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let policy = match string(h, one(args)?, "whenBlocked:")?.as_str() {
        "hold" => WhenBlocked::Hold,
        "reverse" => WhenBlocked::Reverse,
        "crush" => return Err("crush needs an amount: whenBlocked: 'crush' damage: 40".into()),
        other => return Err(format!("no such policy '{other}': hold, reverse or crush")),
    };
    g.set_when_blocked(mover_of(r)?, policy)
        .map_err(|e| mover_error(e, ""))?;
    nil()
}

fn when_blocked_damage(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let &[policy, damage] = args else {
        return Err("whenBlocked:damage: wants a policy and an amount".into());
    };
    if string(h, policy, "whenBlocked:")? != "crush" {
        return Err("only 'crush' takes damage:".into());
    }
    let when = WhenBlocked::Crush {
        damage_per_second: number(h, damage, "damage:")?,
    };
    g.set_when_blocked(mover_of(r)?, when)
        .map_err(|e| mover_error(e, ""))?;
    nil()
}

fn has_player(g: &mut Game, _: &mut Heap, r: Value, _: &[Value]) -> Res {
    let inside = g.script.inside.get(zone_of(r)? as usize);
    val(Value::Bool(inside.copied().unwrap_or(false)))
}

/// Registers a block to run on `trigger`.
pub(crate) fn handler(g: &mut Game, trigger: Trigger, args: &[Value], once: bool) -> Res {
    let block = one(args)?;
    if g.script.handlers.len() >= MAX_HANDLERS {
        return Err(format!("too many handlers: the limit is {MAX_HANDLERS}"));
    }
    g.script.handlers.push(Handler {
        trigger,
        block,
        once,
        done: false,
    });
    nil()
}
