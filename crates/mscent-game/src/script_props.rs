//! Game functions about props and factions.

use crate::enemy::Faction;
use crate::game::Game;
use crate::prop::{Prop, PropKind, PropMesh};
use crate::script::Trigger;
use crate::script_api::{
    ENEMY, GROUP, MOVER, PROP, PROP_KIND, Res, handler, nil, number, one, string, val,
};
use mscent_math::Vec3;
use mscent_script::{Heap, Native, Value};

/// Most props in a run; destroyed ones keep their slot.
const MAX_PROPS: usize = 1024;
/// Most prop kinds a script can define.
const MAX_PROP_KINDS: usize = 64;
/// Largest health or radius a prop can have.
const MAX_PROP_STAT: f32 = 1.0e6;
/// Longest a use can ask the key to be held.
const MAX_HOLD_SECONDS: f32 = 3600.0;

pub(crate) const NATIVES: &[(&str, Native<Game>)] = &[
    ("defineProp:", define_prop),
    ("mesh:", set_mesh),
    ("onPickup:", on_pickup),
    ("onUse:hold:", on_use),
    ("onHit:", on_hit),
    ("onDestroyed:", on_destroyed),
    ("destroy", destroy),
    ("setFaction:", set_faction),
];

pub(crate) fn prop_of(v: Value) -> Result<usize, String> {
    match v {
        Value::Handle(PROP, i) => Ok(i as usize),
        _ => Err("this message is for a prop from spawn:at:tag:".into()),
    }
}

fn prop_mut(g: &mut Game, v: Value) -> Result<&mut Prop, String> {
    let i = prop_of(v)?;
    g.props.get_mut(i).ok_or_else(|| "no such prop".to_string())
}

/// Places a prop of a named kind. `None` if no prop kind has that name.
pub(crate) fn spawn_prop(
    g: &mut Game,
    kind: &str,
    pos: Vec3,
    tag: u16,
) -> Option<Result<Value, String>> {
    let found = g.script.prop_kinds.iter().find(|(n, _)| n == kind)?;
    let kind = found.1;
    if g.props.len() >= MAX_PROPS {
        return Some(Err(format!("too many props: the limit is {MAX_PROPS}")));
    }
    let index = g.props.len();
    g.props.push(Prop::new(pos, &kind));
    g.script.prop_tags.push(vec![tag]);
    g.events.push(crate::Event::PropSpawned(index));
    Some(Ok(Value::Handle(PROP, index as u32)))
}

fn define_prop(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let name = string(h, one(args)?, "a kind")?;
    if name.is_empty() || name.len() > 32 {
        return Err("a kind must be 1 to 32 bytes long".into());
    }
    let taken = g.script.kinds.iter().any(|(n, _, _)| *n == name);
    let kinds = &mut g.script.prop_kinds;
    if taken || kinds.iter().any(|(n, _)| *n == name) {
        return Err(format!("kind '{name}' is already defined"));
    }
    if kinds.len() >= MAX_PROP_KINDS {
        return Err(format!(
            "too many prop kinds: the limit is {MAX_PROP_KINDS}"
        ));
    }
    let start = PropKind {
        mesh: PropMesh::Crate,
        radius: 0.75,
        hp: 0,
    };
    kinds.push((name, start));
    val(Value::Handle(PROP_KIND, (kinds.len() - 1) as u32))
}

fn prop_kind_mut(g: &mut Game, r: Value) -> Result<&mut PropKind, String> {
    let Value::Handle(PROP_KIND, i) = r else {
        return Err("this message is for a prop kind from defineProp:".into());
    };
    let kinds = g.script.prop_kinds.get_mut(i as usize);
    kinds
        .map(|k| &mut k.1)
        .ok_or_else(|| "no such kind".to_string())
}

fn set_mesh(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let name = string(h, one(args)?, "mesh:")?;
    let mesh = PropMesh::from_name(&name)
        .ok_or_else(|| format!("no mesh '{name}': switch, keycard, terminal or crate"))?;
    prop_kind_mut(g, r)?.mesh = mesh;
    val(r)
}

/// `hp:` when sent to a prop kind.
pub(crate) fn set_prop_hp(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let v = number(h, one(args)?, "hp:")?;
    if !(0.0..=MAX_PROP_STAT).contains(&v) {
        return Err(format!("the value must be between 0 and {MAX_PROP_STAT}"));
    }
    prop_kind_mut(g, r)?.hp = v as i32;
    val(r)
}

/// `radius:` when sent to a prop kind.
pub(crate) fn set_prop_radius(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let v = number(h, one(args)?, "radius:")?;
    if v <= 0.0 || v > MAX_PROP_STAT {
        return Err("radius: must be above zero".into());
    }
    prop_kind_mut(g, r)?.radius = v;
    val(r)
}

fn on_pickup(g: &mut Game, _: &mut Heap, r: Value, args: &[Value]) -> Res {
    let i = prop_of(r)?;
    prop_mut(g, r)?.pickup = true;
    handler(g, Trigger::PropPicked(i as u32), args, false)
}

fn on_use(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let &[block, hold] = args else {
        return Err("onUse:hold: wants a block and a time".into());
    };
    let i = prop_of(r)?;
    let hold = number(h, hold, "hold:")?;
    if !(0.0..=MAX_HOLD_SECONDS).contains(&hold) {
        return Err("hold: must be between 0 and an hour".into());
    }
    prop_mut(g, r)?.use_hold = Some(hold);
    handler(g, Trigger::PropUsed(i as u32), &[block], false)
}

/// For a mover, any shot that strikes it; for a prop, likewise, which also
/// makes one with no health stop shots.
fn on_hit(g: &mut Game, _: &mut Heap, r: Value, args: &[Value]) -> Res {
    match r {
        Value::Handle(MOVER, i) => handler(g, Trigger::Hit(i), args, false),
        Value::Handle(PROP, i) => {
            prop_mut(g, r)?.hit_reports = true;
            handler(g, Trigger::PropHit(i), args, false)
        }
        _ => Err("onHit: is for a mover or a prop".into()),
    }
}

fn on_destroyed(g: &mut Game, _: &mut Heap, r: Value, args: &[Value]) -> Res {
    match r {
        Value::Handle(PROP, i) => handler(g, Trigger::PropDestroyed(i), args, false),
        Value::Handle(ENEMY, i) => handler(g, Trigger::EnemyDied(i), args, false),
        _ => Err("onDestroyed: is for a prop or an enemy".into()),
    }
}

/// Removes a prop or kills an enemy, with the same events as a shot.
fn destroy(g: &mut Game, _: &mut Heap, r: Value, _: &[Value]) -> Res {
    match r {
        Value::Handle(PROP, i) => {
            let prop = prop_mut(g, r)?;
            if prop.present {
                prop.present = false;
                g.events.push(crate::Event::PropDestroyed(i as usize));
            }
        }
        Value::Handle(ENEMY, i) => {
            let enemy = g.enemies.get_mut(i as usize).ok_or("no such enemy")?;
            if enemy.damage(i32::MAX) {
                g.events.push(crate::Event::EnemyKilled(i as usize));
            }
        }
        _ => return Err("destroy is for a prop or an enemy".into()),
    }
    nil()
}

fn set_faction(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    let faction = match string(h, one(args)?, "setFaction:")?.as_str() {
        "friendly" => Faction::Friendly,
        "hostile" => Faction::Hostile,
        other => return Err(format!("no such faction '{other}': friendly or hostile")),
    };
    match r {
        Value::Handle(GROUP, tag) => {
            let list: Vec<usize> = g.tag_enemies(tag as u16).collect();
            for i in list {
                g.set_enemy_faction(i, faction);
            }
        }
        Value::Handle(ENEMY, i) => g.set_enemy_faction(i as usize, faction),
        _ => return Err("setFaction: is for a tagged group or an enemy".into()),
    }
    nil()
}
