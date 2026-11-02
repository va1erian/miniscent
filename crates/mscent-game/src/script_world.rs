//! Game functions about enemies, tagged groups, the player and the level's end.

use crate::enemy::{Enemy, EnemyStats};
use crate::game::Game;
use crate::player::MAX_HEALTH;
use crate::script::{ScriptEnd, Trigger};
use crate::script_api::{
    ENEMY, GROUP, KIND, MARKER, PLAYER, PROP, PROP_KIND, Res, handler, nil, number, one, point,
    string, val,
};
use crate::script_props::{set_prop_hp, set_prop_radius, spawn_prop};
use mscent_collide::STEP_HZ;
use mscent_math::Vec3;
use mscent_script::{Flow, Heap, Native, Value};

/// Most distinct tags in a run.
const MAX_TAGS: usize = 256;
/// Most enemies alive at once through spawning. Each one runs a sight ray per
/// step, so this is what a hostile script can cost the frame.
const MAX_ALIVE_SPAWNED: usize = 256;
/// Most enemies a run can ever spawn; dead ones keep their slot.
const MAX_SPAWNED: usize = 4096;
/// Most kinds a script can define, on top of the built-in ones.
const MAX_KINDS: usize = 64;
/// Most items the player can hold.
const MAX_ITEMS: usize = 64;
/// Longest name of a tag, kind or item.
const MAX_NAME_BYTES: usize = 32;
/// Largest value a tuning setter accepts, so a typo cannot overflow later.
const MAX_STAT: f32 = 1.0e6;

pub(crate) const NATIVES: &[(&str, Native<Game>)] = &[
    ("tagged:", tagged),
    ("alive", alive),
    ("members", members),
    ("onEachDestroyed:", on_each_destroyed),
    ("onAllDestroyed:", on_all_destroyed),
    ("enable", |g, _, r, _| set_enabled(g, r, true)),
    ("disable", |g, _, r, _| set_enabled(g, r, false)),
    ("spawn:at:tag:", spawn_one),
    ("spawnAll:at:tag:", spawn_all),
    ("defineEnemy:base:", define_enemy),
    ("hp:", |g, h, r, a| {
        if matches!(r, Value::Handle(PROP_KIND, _)) {
            return set_prop_hp(g, h, r, a);
        }
        set_stat(g, h, r, a, |s, v| s.max_hp = v as i32)
    }),
    ("speed:", |g, h, r, a| {
        set_stat(g, h, r, a, |s, v| s.speed = v)
    }),
    ("radius:", |g, h, r, a| {
        if matches!(r, Value::Handle(PROP_KIND, _)) {
            return set_prop_radius(g, h, r, a);
        }
        set_radius(g, h, r, a)
    }),
    ("sightRange:", |g, h, r, a| {
        set_stat(g, h, r, a, |s, v| s.sight_range = v)
    }),
    ("attackRange:", |g, h, r, a| {
        set_stat(g, h, r, a, |s, v| s.attack_range = v)
    }),
    ("damage:", |g, h, r, a| {
        set_stat(g, h, r, a, |s, v| s.attack_damage = v as i32)
    }),
    ("cooldown:", |g, h, r, a| {
        set_stat(g, h, r, a, |s, v| {
            s.attack_cooldown = (v * STEP_HZ as f32) as u32
        })
    }),
    ("player", |_, _, _, _| val(Value::Handle(PLAYER, 0))),
    ("hp", hp),
    ("heal:", heal),
    ("give:", give),
    ("has:", has),
    ("pos", pos),
    ("say:", say),
    ("win", |g, _, _, _| finish(g, ScriptEnd::Win)),
    ("lose", |g, _, _, _| finish(g, ScriptEnd::Lose)),
    ("randomFrom:to:", random),
];

fn short_name(h: &Heap, v: Value, what: &str) -> Result<String, String> {
    let name = string(h, v, what)?;
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(format!("{what} must be 1 to {MAX_NAME_BYTES} bytes long"));
    }
    Ok(name)
}

impl Game {
    pub(crate) fn intern_tag(&mut self, tag: &str) -> Result<u16, String> {
        let tags = &mut self.script.tags;
        if let Some(i) = tags.iter().position(|t| t == tag) {
            return Ok(i as u16);
        }
        if tags.len() >= MAX_TAGS {
            return Err(format!("too many tags: the limit is {MAX_TAGS}"));
        }
        tags.push(tag.to_string());
        Ok((tags.len() - 1) as u16)
    }

    /// Enemies and props with this tag still there.
    pub(crate) fn tag_alive(&self, tag: u16) -> usize {
        self.tag_enemies(tag).count() + self.tag_props(tag).count()
    }

    fn tag_props(&self, tag: u16) -> impl Iterator<Item = usize> + '_ {
        let tags = self.script.prop_tags.iter().enumerate();
        tags.filter(move |(_, t)| t.contains(&tag))
            .map(|(i, _)| i)
            .filter(|&i| self.props.get(i).is_some_and(|p| p.present))
    }

    pub(crate) fn tag_enemies(&self, tag: u16) -> impl Iterator<Item = usize> + '_ {
        let tags = self.script.enemy_tags.iter().enumerate();
        tags.filter(move |(_, t)| t.contains(&tag))
            .map(|(i, _)| i)
            .filter(|&i| self.enemies.get(i).is_some_and(Enemy::alive))
    }
}

fn tagged(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let tag = short_name(h, one(args)?, "a tag")?;
    val(Value::Handle(GROUP, u32::from(g.intern_tag(&tag)?)))
}

fn alive(g: &mut Game, _: &mut Heap, r: Value, _: &[Value]) -> Res {
    let count = match r {
        Value::Handle(GROUP, tag) => g.tag_alive(tag as u16),
        Value::Handle(ENEMY, i) => usize::from(g.enemies.get(i as usize).is_some_and(Enemy::alive)),
        Value::Handle(PROP, i) => usize::from(g.props.get(i as usize).is_some_and(|p| p.present)),
        _ => return Err("alive is for a tagged group or an enemy".into()),
    };
    val(Value::Num(count as f32))
}

fn members(g: &mut Game, h: &mut Heap, r: Value, _: &[Value]) -> Res {
    let Value::Handle(GROUP, tag) = r else {
        return Err("members is for a tagged group".into());
    };
    let enemies = g
        .tag_enemies(tag as u16)
        .map(|i| Value::Handle(ENEMY, i as u32));
    let props = g
        .tag_props(tag as u16)
        .map(|i| Value::Handle(PROP, i as u32));
    let list = enemies.chain(props).collect();
    h.new_array(list).map(Flow::Val)
}

fn on_each_destroyed(g: &mut Game, _: &mut Heap, r: Value, args: &[Value]) -> Res {
    let trigger = match r {
        Value::Handle(GROUP, tag) => Trigger::Each(tag as u16),
        Value::Handle(ENEMY, i) => Trigger::EnemyDied(i),
        Value::Handle(PROP, i) => Trigger::PropDestroyed(i),
        _ => return Err("onEachDestroyed: is for a tagged group, an enemy or a prop".into()),
    };
    handler(g, trigger, args, false)
}

fn on_all_destroyed(g: &mut Game, _: &mut Heap, r: Value, args: &[Value]) -> Res {
    let Value::Handle(GROUP, tag) = r else {
        return Err("onAllDestroyed: is for a tagged group".into());
    };
    handler(g, Trigger::All(tag as u16), args, false)
}

fn set_enabled(g: &mut Game, r: Value, on: bool) -> Res {
    match r {
        Value::Handle(GROUP, tag) => {
            let list: Vec<usize> = g.tag_enemies(tag as u16).collect();
            for i in list {
                g.enemies[i].enabled = on;
            }
            let list: Vec<usize> = g.tag_props(tag as u16).collect();
            for i in list {
                g.props[i].enabled = on;
            }
        }
        Value::Handle(ENEMY, i) => {
            let enemy = g.enemies.get_mut(i as usize).ok_or("no such enemy")?;
            enemy.enabled = on;
        }
        Value::Handle(PROP, i) => {
            let prop = g.props.get_mut(i as usize).ok_or("no such prop")?;
            prop.enabled = on;
        }
        _ => return Err("this message is for a tagged group, an enemy or a prop".into()),
    }
    nil()
}

fn spawn_at(g: &mut Game, kind: &str, pos: Vec3, tag: u16) -> Result<Value, String> {
    if !pos.is_finite() || g.arena.bsp.is_solid(pos.to_f64()) {
        return Err(format!("cannot spawn '{kind}' inside solid rock"));
    }
    if let Some(prop) = spawn_prop(g, kind, pos, tag) {
        return prop;
    }
    let found = g.script.kinds.iter().find(|(n, _, _)| n == kind);
    let (base, stats) = found
        .map(|(_, b, s)| (*b, *s))
        .ok_or_else(|| format!("no kind '{kind}'; defineEnemy:base: or defineProp: makes one"))?;
    let spawned = g.enemies.len().saturating_sub(g.arena.enemies.len());
    if spawned >= MAX_SPAWNED {
        return Err(format!(
            "too many spawned enemies: the limit is {MAX_SPAWNED}"
        ));
    }
    let first_spawned = g.arena.enemies.len();
    let alive = g.enemies.iter().skip(first_spawned).filter(|e| e.alive());
    if alive.count() >= MAX_ALIVE_SPAWNED {
        return Err(format!(
            "too many spawned enemies alive: the limit is {MAX_ALIVE_SPAWNED}"
        ));
    }
    let index = g.enemies.len();
    g.enemies.push(Enemy::with_stats(index, pos, base, stats));
    g.script.enemy_tags.push(vec![tag]);
    g.events.push(crate::Event::EnemySpawned(index));
    Ok(Value::Handle(ENEMY, index as u32))
}

fn spawn_one(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[kind, at, tag] = args else {
        return Err("spawn:at:tag: wants a kind, a place and a tag".into());
    };
    let kind = string(h, kind, "the kind")?;
    let pos = point(g, h, at)?;
    let tag = short_name(h, tag, "a tag")?;
    let tag = g.intern_tag(&tag)?;
    spawn_at(g, &kind, pos, tag).map(Flow::Val)
}

fn spawn_all(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[kind, places, tag] = args else {
        return Err("spawnAll:at:tag: wants a kind, places and a tag".into());
    };
    let kind = string(h, kind, "the kind")?;
    let places = h
        .array(places)
        .ok_or("at: must be an array of places")?
        .to_vec();
    let tag = short_name(h, tag, "a tag")?;
    let tag = g.intern_tag(&tag)?;
    for place in places {
        let pos = point(g, h, place)?;
        spawn_at(g, &kind, pos, tag)?;
    }
    nil()
}

/// A new enemy kind, starting from the built-in one numbered `base`.
fn define_enemy(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[name, base] = args else {
        return Err("defineEnemy:base: wants a name and a base".into());
    };
    let name = short_name(h, name, "a kind")?;
    let base = number(h, base, "base:")?;
    if !(0.0..=1.0).contains(&base) || base.fract() != 0.0 {
        return Err("base: must be 0 (drone) or 1 (hunter)".into());
    }
    let kinds = &mut g.script.kinds;
    let taken = g.script.prop_kinds.iter().any(|(n, _)| *n == name);
    if taken || kinds.iter().any(|(n, _, _)| *n == name) {
        return Err(format!("kind '{name}' is already defined"));
    }
    if kinds.len() >= MAX_KINDS {
        return Err(format!("too many enemy kinds: the limit is {MAX_KINDS}"));
    }
    let base = base as u8;
    kinds.push((name, base, crate::stats_for(base)));
    val(Value::Handle(KIND, (kinds.len() - 1) as u32))
}

fn kind_stats(g: &mut Game, r: Value) -> Result<&mut EnemyStats, String> {
    let Value::Handle(KIND, i) = r else {
        return Err("this message is for an enemy kind from defineEnemy:base:".into());
    };
    let kind = g.script.kinds.get_mut(i as usize).ok_or("no such kind")?;
    Ok(&mut kind.2)
}

fn set_stat(
    g: &mut Game,
    h: &mut Heap,
    r: Value,
    args: &[Value],
    apply: fn(&mut EnemyStats, f32),
) -> Res {
    let v = number(h, one(args)?, "the value")?;
    if !(0.0..=MAX_STAT).contains(&v) {
        return Err(format!("the value must be between 0 and {MAX_STAT}"));
    }
    apply(kind_stats(g, r)?, v);
    val(r)
}

/// A radius of zero would make an enemy no hitscan could ever strike.
fn set_radius(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    if number(h, one(args)?, "radius:")? <= 0.0 {
        return Err("radius: must be above zero".into());
    }
    set_stat(g, h, r, args, |s, v| s.radius = v)
}

fn hp(g: &mut Game, _: &mut Heap, r: Value, _: &[Value]) -> Res {
    let hp = match r {
        Value::Handle(PLAYER, _) => g.player.health,
        Value::Handle(ENEMY, i) => g.enemies.get(i as usize).ok_or("no such enemy")?.hp,
        Value::Handle(PROP, i) => g.props.get(i as usize).ok_or("no such prop")?.hp,
        _ => return Err("hp is for the player, an enemy or a prop".into()),
    };
    val(Value::Num(hp as f32))
}

fn heal(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    if !matches!(r, Value::Handle(PLAYER, _)) {
        return Err("heal: is for the player".into());
    }
    let amount = number(h, one(args)?, "heal:")?;
    if amount < 0.0 {
        return Err("heal: must not be negative".into());
    }
    let health = g.player.health.saturating_add(amount as i32);
    g.player.health = health.min(MAX_HEALTH);
    nil()
}

fn give(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    if !matches!(r, Value::Handle(PLAYER, _)) {
        return Err("give: is for the player".into());
    }
    let item = short_name(h, one(args)?, "an item")?;
    let items = &mut g.script.inventory;
    if !items.contains(&item) {
        if items.len() >= MAX_ITEMS {
            return Err(format!("too many items: the limit is {MAX_ITEMS}"));
        }
        items.push(item);
    }
    nil()
}

fn has(g: &mut Game, h: &mut Heap, r: Value, args: &[Value]) -> Res {
    if !matches!(r, Value::Handle(PLAYER, _)) {
        return Err("has: is for the player".into());
    }
    let item = string(h, one(args)?, "an item")?;
    val(Value::Bool(g.has_item(&item)))
}

fn pos(g: &mut Game, h: &mut Heap, r: Value, _: &[Value]) -> Res {
    let p = match r {
        Value::Handle(PLAYER, _) => g.player.pos(),
        Value::Handle(ENEMY, i) => g.enemies.get(i as usize).ok_or("no such enemy")?.pos(),
        Value::Handle(PROP, i) => g.props.get(i as usize).ok_or("no such prop")?.pos,
        Value::Handle(MARKER, _) => point(g, h, r)?,
        _ => return Err("pos is for the player, an enemy, a prop or a marker".into()),
    };
    let items = vec![Value::Num(p.x), Value::Num(p.y), Value::Num(p.z)];
    h.new_array(items).map(Flow::Val)
}

fn say(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let text = h.text(one(args)?);
    g.script.say(text);
    nil()
}

fn finish(g: &mut Game, end: ScriptEnd) -> Res {
    g.script.end.get_or_insert(end);
    nil()
}

fn random(g: &mut Game, h: &mut Heap, _: Value, args: &[Value]) -> Res {
    let &[from, to] = args else {
        return Err("randomFrom:to: wants two numbers".into());
    };
    let (from, to) = (number(h, from, "randomFrom:")?, number(h, to, "to:")?);
    val(Value::Num(g.script.rng.range_f32(from, to)))
}
