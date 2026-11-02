//! Level scripts at play (M9.5): loading one, starting handlers for this step's
//! events and running due tasks. The game functions scripts can call are in
//! `script_api.rs` and `script_world.rs`.
//!
//! `Game` is the VM's host, so a game function reads and changes the game
//! directly and can report a bad request as an error on the script's own line.
//! The VM is taken out of the game while it runs, which is what lets it have
//! `&mut Game`. A game with no script pays for one `Option` check per step.

use crate::arena::Thing;
use crate::game::{Event, Game};
use crate::script_api::{ENEMY, MOVER, PLAYER, natives};
use mscent_math::rng::Rng;
use mscent_script::{Host, NameCheck, ScriptError, Source, Value, Vm};

/// Most handlers a script may register.
pub const MAX_HANDLERS: usize = 1024;
/// Most runtime errors or `say:` messages kept until the host takes them. A
/// script that produces more loses the newest, not memory.
const MAX_PENDING: usize = 64;
/// Seed of the generator behind `randomFrom:to:`, fixed so replays match.
const RANDOM_SEED: u64 = 0x5CA1_AB1E;

/// Functions written in the language that need the game's natives, so they
/// cannot live in the script crate's own prelude.
const GAME_LIBRARY: &str = "
define waitArrived: m [ [m moving] whileTrue: [wait: 0.05] ]
define waitDestroyed: g [ [g alive > 0] whileTrue: [wait: 0.1] ]
";

/// What wakes a handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    ZoneEnter(u32),
    ZoneExit(u32),
    Arrived(u32),
    Hit(u32),
    EnemyDied(u32),
    /// Any enemy with this tag died.
    Each(u16),
    /// The last living enemy with this tag died.
    All(u16),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Handler {
    pub trigger: Trigger,
    pub block: Value,
    pub once: bool,
    /// Ended by an error, or a `once` handler that has run.
    pub done: bool,
}

/// How a script ended the level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScriptEnd {
    Win,
    Lose,
}

/// Everything about a script's run that lives outside the VM. A restart makes
/// a new one.
#[derive(Clone, Debug)]
pub(crate) struct ScriptState {
    pub handlers: Vec<Handler>,
    pub tags: Vec<String>,
    /// Parallel to `Game::enemies`: the tags of each.
    pub enemy_tags: Vec<Vec<u16>>,
    /// Enemy kinds a script can spawn, by name, with the base mesh kind.
    pub kinds: Vec<(String, u8, crate::EnemyStats)>,
    pub inventory: Vec<String>,
    pub messages: Vec<String>,
    pub errors: Vec<ScriptError>,
    /// Parallel to the arena's zones: whether the player is inside.
    pub inside: Vec<bool>,
    pub rng: Rng,
    pub end: Option<ScriptEnd>,
    /// Which handler each started task belongs to, so an error can end it.
    fired: Vec<(u64, usize)>,
}

impl ScriptState {
    pub(crate) fn new(zones: usize, enemies: usize) -> Self {
        Self {
            handlers: Vec::new(),
            tags: Vec::new(),
            enemy_tags: vec![Vec::new(); enemies],
            kinds: vec![
                ("drone".to_string(), 0, crate::stats_for(0)),
                ("hunter".to_string(), 1, crate::stats_for(1)),
            ],
            inventory: Vec::new(),
            messages: Vec::new(),
            errors: Vec::new(),
            inside: vec![false; zones],
            rng: Rng::new(RANDOM_SEED),
            end: None,
            fired: Vec::new(),
        }
    }

    pub(crate) fn say(&mut self, text: String) {
        if self.messages.len() < MAX_PENDING {
            self.messages.push(text);
        }
    }

    fn note_error(&mut self, e: ScriptError) {
        let owner = self.fired.iter().find(|(task, _)| Some(*task) == e.task);
        if let Some(h) = owner.and_then(|(_, handler)| self.handlers.get_mut(*handler)) {
            h.done = true;
        }
        if self.errors.len() < MAX_PENDING {
            self.errors.push(e);
        }
    }
}

impl Host for Game {
    fn roots(&self, visit: &mut dyn FnMut(Value)) {
        self.script.handlers.iter().for_each(|h| visit(h.block));
    }
}

impl Game {
    /// Compiles `sources`, in order: libraries, the level, then a test if any.
    /// On success the level restarts on the script, and what comes back is the
    /// compiler's warnings. On failure the game is unchanged.
    pub fn load_script(
        &mut self,
        sources: &[Source],
    ) -> Result<Vec<ScriptError>, Vec<ScriptError>> {
        let all: Vec<Source> = std::iter::once(Source {
            name: "<game>",
            text: GAME_LIBRARY,
        })
        .chain(sources.iter().copied())
        .collect();
        let loaded = {
            let names = self.arena.names.selector_names();
            let checks: Vec<NameCheck> = names
                .iter()
                .map(|(selector, list)| NameCheck {
                    selector,
                    names: list,
                })
                .collect();
            Vm::load(&all, &natives(), &checks)?
        };
        self.script_program = Some(Box::new(loaded.vm));
        self.restart();
        Ok(loaded.warnings)
    }

    /// Starts the script over, from a fresh copy of the loaded program.
    pub(crate) fn start_script(&mut self) {
        self.script = ScriptState::new(self.arena.zones.len(), self.enemies.len());
        self.tag_level_enemies();
        let Some(mut vm) = self.script_program.clone() else {
            self.vm = None;
            return;
        };
        vm.start(self);
        self.drain_errors(&mut vm);
        self.vm = Some(vm);
    }

    fn tag_level_enemies(&mut self) {
        let tagged: Vec<(String, usize)> = self
            .arena
            .names
            .tagged
            .iter()
            .flat_map(|(tag, things)| {
                things.iter().filter_map(move |t| match t {
                    Thing::Enemy(i) => Some((tag.clone(), *i)),
                    _ => None,
                })
            })
            .collect();
        for (tag, enemy) in tagged {
            if let (Ok(id), true) = (self.intern_tag(&tag), enemy < self.script.enemy_tags.len()) {
                self.script.enemy_tags[enemy].push(id);
            }
        }
    }

    /// The runtime errors since the last call, oldest first.
    pub fn take_script_errors(&mut self) -> Vec<ScriptError> {
        std::mem::take(&mut self.script.errors)
    }

    /// What scripts have said since the last call, oldest first.
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.script.messages)
    }

    /// Whether the player holds a key given by a script.
    pub fn has_item(&self, name: &str) -> bool {
        self.script.inventory.iter().any(|i| i == name)
    }

    pub fn has_script(&self) -> bool {
        self.vm.is_some()
    }

    /// The script's part of a step: zones, then a task for every handler of an
    /// event since `mark`, then every task that is due.
    pub(crate) fn run_script(&mut self, mark: usize) {
        let Some(mut vm) = self.vm.take() else {
            return;
        };
        self.update_zones();
        let events = self.events.get(mark..).map(<[Event]>::to_vec);
        for event in events.unwrap_or_default() {
            self.start_handlers(&mut vm, event);
        }
        vm.step(self);
        self.drain_errors(&mut vm);
        self.vm = Some(vm);
    }

    fn update_zones(&mut self) {
        let p = self.player.pos();
        for (i, zone) in self.arena.zones.iter().enumerate() {
            let d = p - zone.pos;
            let now =
                d.x.abs() <= zone.half.x && d.y.abs() <= zone.half.y && d.z.abs() <= zone.half.z;
            let Some(was) = self.script.inside.get_mut(i) else {
                continue;
            };
            if now != *was {
                *was = now;
                self.events.push(if now {
                    Event::ZoneEntered(i)
                } else {
                    Event::ZoneLeft(i)
                });
            }
        }
    }

    fn start_handlers(&mut self, vm: &mut Vm<Game>, event: Event) {
        let player = Value::Handle(PLAYER, 0);
        let handle = |kind, i: usize| Value::Handle(kind, i as u32);
        match event {
            Event::ZoneEntered(i) => self.fire_trigger(vm, Trigger::ZoneEnter(i as u32), player),
            Event::ZoneLeft(i) => self.fire_trigger(vm, Trigger::ZoneExit(i as u32), player),
            Event::MoverArrived(i) => {
                self.fire_trigger(vm, Trigger::Arrived(i as u32), handle(MOVER, i))
            }
            Event::MoverHit(i) => self.fire_trigger(vm, Trigger::Hit(i as u32), handle(MOVER, i)),
            Event::EnemyKilled(i) => self.enemy_died(vm, i),
            _ => {}
        }
    }

    fn enemy_died(&mut self, vm: &mut Vm<Game>, enemy: usize) {
        let subject = Value::Handle(ENEMY, enemy as u32);
        self.fire_trigger(vm, Trigger::EnemyDied(enemy as u32), subject);
        let tags = self
            .script
            .enemy_tags
            .get(enemy)
            .cloned()
            .unwrap_or_default();
        for tag in tags {
            self.fire_trigger(vm, Trigger::Each(tag), subject);
            if self.tag_alive(tag) == 0 {
                self.fire_trigger(vm, Trigger::All(tag), subject);
            }
        }
    }

    /// Starts a task for each handler of `trigger`. One that takes a parameter
    /// gets `subject`, what the event was about.
    fn fire_trigger(&mut self, vm: &mut Vm<Game>, trigger: Trigger, subject: Value) {
        for i in 0..self.script.handlers.len() {
            let Some(&h) = self.script.handlers.get(i) else {
                continue;
            };
            if h.trigger != trigger || h.done {
                continue;
            }
            let wants_subject = vm.block_params(h.block) == Some(1);
            let args: &[Value] = if wants_subject { &[subject] } else { &[] };
            if let Some(task) = vm.fire(h.block, args) {
                self.note_fired(vm, task, i);
            }
            if h.once {
                self.script.handlers[i].done = true;
            }
        }
    }

    fn note_fired(&mut self, vm: &Vm<Game>, task: u64, handler: usize) {
        let fired = &mut self.script.fired;
        // Bounded by the task cap: finished tasks can no longer report errors.
        if fired.len() >= 2 * mscent_script::MAX_TASKS {
            fired.retain(|(t, _)| vm.task_alive(*t));
        }
        fired.push((task, handler));
    }

    fn drain_errors(&mut self, vm: &mut Vm<Game>) {
        for e in vm.take_errors() {
            self.script.note_error(e);
        }
    }
}
