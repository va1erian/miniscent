//! A stand-in for the game: natives that log what scripts ask for, and hold
//! the blocks handed to `onEnter:` and friends. Proves the language and the
//! timing of waits, not gameplay.
#![allow(dead_code)]

use mscent_script::{Flow, Heap, Host, NameCheck, Native, ScriptError, Source, Value, Vm};

pub const MOVER: u8 = 1;
pub const OTHER: u8 = 2;
pub const NAMES: &[&str] = &[
    "vault_door",
    "red_door",
    "red_door_area",
    "side_door",
    "side_door_area",
    "trapdoor",
    "trap_plate",
    "trap_hinge",
    "crusher",
    "lift",
    "hall_exit",
    "hall_entry",
    "hall",
    "red_key",
    "sec_terminal",
    "vault_switch",
    "sec_drones",
    "sec_turrets",
    "wave1",
    "wave2",
    "hall_wave1",
    "hall_wave2",
];

#[derive(Clone, Default)]
pub struct Stub {
    pub log: Vec<String>,
    pub handlers: Vec<(String, Value)>,
    pub alive: f32,
    pub tick: u64,
    /// Values the host keeps and shows the collector.
    pub rooted: Vec<Value>,
    /// Values the host keeps and forgets to show the collector.
    pub unrooted: Vec<Value>,
    /// How often `note:` was called with each number.
    pub counts: Vec<u32>,
}

impl Host for Stub {
    fn roots(&self, visit: &mut dyn FnMut(Value)) {
        self.handlers.iter().for_each(|(_, v)| visit(*v));
        self.rooted.iter().for_each(|v| visit(*v));
    }
}

fn handle(h: &mut Heap, kind: u8, v: Value) -> Result<Flow, String> {
    let name = h.text(v);
    let id = NAMES
        .iter()
        .position(|n| *n == name)
        .ok_or_else(|| format!("no such name '{name}'"))?;
    Ok(Flow::Val(Value::Handle(kind, id as u32)))
}

fn nm(v: Value) -> &'static str {
    match v {
        Value::Handle(_, i) => NAMES.get(i as usize).copied().unwrap_or("?"),
        _ => "?",
    }
}

fn on(h: &mut Stub, ev: &str, r: Value, b: Value) -> Result<Flow, String> {
    h.handlers.push((format!("{}.{ev}", nm(r)), b));
    Ok(Flow::Val(Value::Nil))
}

pub fn say(h: &mut Stub, line: String) -> Result<Flow, String> {
    h.log.push(format!("[{:>4}] {line}", h.tick));
    Ok(Flow::Val(Value::Nil))
}

fn nil() -> Result<Flow, String> {
    Ok(Flow::Val(Value::Nil))
}

fn secs(v: Value, what: &str) -> Result<Flow, String> {
    match v {
        Value::Num(n) => Ok(Flow::Wait(n)),
        _ => Err(format!("{what} wants a number")),
    }
}

/// The game-like natives the design's examples use.
pub fn natives() -> Vec<(&'static str, Native<Stub>)> {
    vec![
        ("mover:", |_, h, _, a| handle(h, MOVER, a[0])),
        ("zone:", |_, h, _, a| handle(h, OTHER, a[0])),
        ("marker:", |_, h, _, a| handle(h, OTHER, a[0])),
        ("tagged:", |_, h, _, a| handle(h, OTHER, a[0])),
        ("markers:", |_, h, _, _| {
            h.new_array(vec![Value::Handle(OTHER, 0); 3]).map(Flow::Val)
        }),
        ("player", |_, _, _, _| {
            Ok(Flow::Val(Value::Handle(OTHER, 999)))
        }),
        ("x:y:z:", |_, h, _, a| {
            h.new_array(a.to_vec()).map(Flow::Val)
        }),
        ("say:", |s, h, _, a| {
            let t = h.text(a[0]);
            say(s, format!("say {t}"))
        }),
        ("pose:move:", |s, h, r, a| {
            let t = h.text(a[0]);
            say(s, format!("pose {} {t}", nm(r)))
        }),
        ("pose:turn:degrees:about:", |s, h, r, a| {
            let t = h.text(a[0]);
            say(s, format!("pose {} {t}", nm(r)))
        }),
        ("moveTo:in:", |s, h, r, a| {
            let t = h.text(a[0]);
            say(s, format!("move {} {t}", nm(r)))
        }),
        ("moving", |_, _, _, _| Ok(Flow::Val(Value::Bool(false)))),
        ("poseNow", |_, h, _, _| {
            h.new_str("closed".to_string()).map(Flow::Val)
        }),
        ("whenBlocked:", |_, _, _, _| nil()),
        ("whenBlocked:damage:", |_, _, _, _| nil()),
        ("defineProp:", |_, _, _, _| {
            Ok(Flow::Val(Value::Handle(OTHER, 0)))
        }),
        ("defineEnemy:base:", |_, _, _, _| {
            Ok(Flow::Val(Value::Handle(OTHER, 0)))
        }),
        ("mesh:", |_, _, _, _| nil()),
        ("radius:", |_, _, _, _| nil()),
        ("hp:", |_, _, _, _| nil()),
        ("speed:", |_, _, _, _| nil()),
        ("members", |_, h, _, _| {
            h.new_array(vec![Value::Handle(OTHER, 0); 3]).map(Flow::Val)
        }),
        ("teleport:distance:", |s, _, _, _| say(s, "teleport".into())),
        ("aimAt:", |s, _, _, _| say(s, "aim".into())),
        ("fireFor:", |_, _, _, a| secs(a[0], "fireFor:")),
        ("shot:", |s, h, _, a| {
            let t = h.text(a[0]);
            say(s, format!("shot {t}"))
        }),
        ("assert:because:", |s, h, _, a| {
            let (ok, why) = (h.text(a[0]), h.text(a[1]));
            say(s, format!("assert {ok}: {why}"))
        }),
        ("hasPlayer", |_, _, _, _| Ok(Flow::Val(Value::Bool(false)))),
        ("lose", |s, _, _, _| say(s, "lose".into())),
        ("alive", |s, _, _, _| Ok(Flow::Val(Value::Num(s.alive)))),
        ("hp", |_, _, _, _| Ok(Flow::Val(Value::Num(30.0)))),
        ("has:", |_, _, _, _| Ok(Flow::Val(Value::Bool(false)))),
        ("give:", |s, h, _, a| {
            let t = h.text(a[0]);
            say(s, format!("give {t}"))
        }),
        ("heal:", |s, _, _, _| say(s, "heal".into())),
        ("setFaction:", |s, h, r, a| {
            let t = h.text(a[0]);
            say(s, format!("faction {} {t}", nm(r)))
        }),
        ("disable", |s, _, r, _| say(s, format!("disable {}", nm(r)))),
        ("spawn:at:tag:", |s, h, _, a| {
            let t = h.text(a[0]);
            say(s, format!("spawn {t}"))?;
            Ok(Flow::Val(Value::Handle(OTHER, 13)))
        }),
        ("spawnAll:at:tag:", |s, h, _, a| {
            let t = h.text(a[0]);
            say(s, format!("spawnAll {t}"))
        }),
        ("onEnter:", |s, _, r, a| on(s, "enter", r, a[0])),
        ("onceEnter:", |s, _, r, a| on(s, "enter", r, a[0])),
        ("onExit:", |s, _, r, a| on(s, "exit", r, a[0])),
        ("onPickup:", |s, _, r, a| on(s, "pickup", r, a[0])),
        ("onEachDestroyed:", |s, _, r, a| on(s, "each", r, a[0])),
        ("onAllDestroyed:", |s, _, r, a| on(s, "all", r, a[0])),
        ("onUse:hold:", |s, _, r, a| on(s, "use", r, a[0])),
        // Small helpers for the language tests.
        ("keep:", |s, _, _, a| {
            s.rooted.push(a[0]);
            nil()
        }),
        ("hold:", |s, _, _, a| {
            s.handlers.push(("held".into(), a[0]));
            nil()
        }),
        ("park:", |_, _, _, a| secs(a[0], "park:")),
        ("stash:", |s, _, _, a| {
            s.unrooted.push(a[0]);
            nil()
        }),
        ("note:", |s, _, _, a| {
            let Value::Num(n) = a[0] else {
                return Err("note: wants a number".into());
            };
            let i = n as usize;
            if s.counts.len() <= i {
                s.counts.resize(i + 1, 0);
            }
            s.counts[i] += 1;
            nil()
        }),
        ("waitnan", |_, _, _, _| Ok(Flow::Wait(f32::NAN))),
    ]
}

pub fn name_checks() -> Vec<NameCheck<'static>> {
    ["mover:", "zone:", "marker:", "tagged:"]
        .iter()
        .map(|s| NameCheck {
            selector: s,
            names: NAMES,
        })
        .collect()
}

/// Loads sources against the stub natives and the level names.
pub fn load(sources: &[(&str, &str)]) -> Result<mscent_script::Loaded<Stub>, Vec<ScriptError>> {
    let list: Vec<Source> = sources
        .iter()
        .map(|&(name, text)| Source { name, text })
        .collect();
    Vm::load(&list, &natives(), &name_checks())
}

pub fn load_errors(sources: &[(&str, &str)]) -> Vec<String> {
    match load(sources) {
        Ok(_) => panic!("expected load errors"),
        Err(es) => es.iter().map(ToString::to_string).collect(),
    }
}

pub fn level(src: &str) -> Result<mscent_script::Loaded<Stub>, Vec<ScriptError>> {
    load(&[("level.st", src)])
}

/// Loads one script and returns the VM, panicking on a load error.
pub fn vm_of(src: &str) -> (Vm<Stub>, Stub) {
    let loaded = level(src).unwrap_or_else(|e| panic!("load failed: {e:?}"));
    (loaded.vm, Stub::default())
}

/// Starts the VM and steps it `ticks` times, calling `each` before every step
/// with the tick that step will be.
pub fn drive(
    vm: &mut Vm<Stub>,
    host: &mut Stub,
    ticks: u64,
    mut each: impl FnMut(u64, &mut Vm<Stub>, &mut Stub),
) {
    vm.start(host);
    for t in 1..=ticks {
        host.tick = t;
        each(t, vm, host);
        vm.step(host);
    }
}

/// Fires the first handler registered under `name`.
pub fn fire(vm: &mut Vm<Stub>, host: &Stub, name: &str) {
    let Some((_, block)) = host.handlers.iter().find(|(n, _)| n == name) else {
        panic!("no handler {name}");
    };
    vm.fire(*block, &[]);
}

/// Loads, runs with no events, and returns the log and errors.
pub fn run_plain(src: &str, ticks: u64) -> (Vec<String>, Vec<String>) {
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, ticks, |_, _, _| {});
    (
        host.log,
        vm.take_errors().iter().map(ToString::to_string).collect(),
    )
}

/// The log with the tick brackets stripped, for tests that do not care when.
pub fn says(log: &[String]) -> Vec<String> {
    log.iter()
        .map(|l| l.split_once("] ").map_or(l.clone(), |x| x.1.to_string()))
        .collect()
}

/// Runs `f` on a thread with a stack smaller than the usual 2 MiB, so a
/// recursion that only just fits a normal stack fails here.
pub fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let t = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(f)
        .expect("thread starts");
    t.join().expect("no panic and no stack overflow")
}

/// Loads against a custom natives table with no level names.
pub fn load_natives(
    natives: &[(&str, Native<Stub>)],
    src: &str,
) -> Result<mscent_script::Loaded<Stub>, Vec<ScriptError>> {
    Vm::load(
        &[Source {
            name: "level.st",
            text: src,
        }],
        natives,
        &[],
    )
}

/// The text of `tests/scripts/<name>.st`.
pub fn script(name: &str) -> String {
    let path = format!("{}/tests/scripts/{name}.st", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}
