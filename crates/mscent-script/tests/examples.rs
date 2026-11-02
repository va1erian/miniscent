//! The design's examples (docs/scripting.md) as tests against the logging
//! stub host, asserting the whole log with the tick each line happens at.

mod common;
use common::*;

struct Run {
    log: Vec<String>,
    errors: Vec<String>,
    tasks: usize,
}

/// Loads the shared libraries and `level` (and `test` when given), starts
/// them, and drives `ticks` steps. `events` fire handlers at ticks, and
/// `alive` changes the stub's live count at a tick.
fn run_example(
    level: &str,
    test: Option<&str>,
    events: &[(u64, &str)],
    alive: &[(u64, f32)],
    ticks: u64,
) -> Run {
    let (wait, doors, level) = (script("lib_wait"), script("lib_doors"), script(level));
    let mut sources = vec![
        ("lib/wait.st", wait.as_str()),
        ("lib/doors.st", doors.as_str()),
        ("level.st", level.as_str()),
    ];
    let test_src = test.map(script);
    if let Some(t) = &test_src {
        sources.push(("level.test.st", t));
    }
    let loaded = load(&sources).unwrap_or_else(|e| panic!("load failed: {e:?}"));
    let (mut vm, mut host) = (
        loaded.vm,
        Stub {
            alive: 2.0,
            ..Stub::default()
        },
    );
    drive(&mut vm, &mut host, ticks, |t, vm, host| {
        for &(_, a) in alive.iter().filter(|x| x.0 == t) {
            host.alive = a;
        }
        for &(_, name) in events.iter().filter(|e| e.0 == t) {
            fire(vm, host, name);
        }
    });
    Run {
        log: host.log.clone(),
        errors: vm.take_errors().iter().map(ToString::to_string).collect(),
        tasks: vm.task_count(),
    }
}

fn strs(log: &[String]) -> Vec<&str> {
    log.iter().map(String::as_str).collect()
}

fn at(log: &[String], text: &str) -> Vec<u64> {
    let hits = log.iter().filter(|l| l.ends_with(text));
    hits.filter_map(|l| l.strip_prefix('[')?.split(']').next()?.trim().parse().ok())
        .collect()
}

#[test]
fn shoot_switches_to_open_a_door() {
    let r = run_example(
        "switches",
        None,
        &[(10, "vault_switch.each"), (20, "vault_switch.all")],
        &[(20, 0.0)],
        400,
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        strs(&r.log),
        [
            "[   0] pose vault_door open",
            "[   0] spawn switch",
            "[   0] spawn switch",
            "[   0] spawn switch",
            "[  10] say 2 left",
            "[  20] move vault_door open",
            "[  20] say Vault unlocked",
        ]
    );
}

#[test]
fn a_keycard_and_a_library_function_that_makes_doors() {
    let events = [
        (50, "red_door_area.enter"),
        (60, "side_door_area.enter"),
        (70, "side_door_area.exit"),
        (80, "red_key.pickup"),
    ];
    let r = run_example("keycard", None, &events, &[], 400);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        strs(&r.log),
        [
            "[   0] spawn red_keycard",
            "[   0] pose red_door open",
            "[   0] pose side_door open",
            "[  50] say Needs the red keycard",
            "[  60] move side_door open",
            "[  70] move side_door closed",
            "[  80] give red",
            "[  80] say Red keycard",
        ]
    );
}

#[test]
fn a_trapdoor_and_a_crusher_that_cycles() {
    let r = run_example("movers", None, &[(30, "trap_plate.enter")], &[], 600);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        strs(&r.log),
        [
            "[   0] pose trapdoor open",
            "[   0] pose crusher down",
            "[   0] move crusher down",
            "[  30] move trapdoor open",
            "[ 120] move crusher closed",
            "[ 240] move crusher down",
            "[ 360] move crusher closed",
        ]
    );
    assert_eq!(r.tasks, 0, "the crusher task ended after two cycles");
}

#[test]
fn a_lift_on_a_timer_and_a_crusher_forever() {
    let r = run_example("movers2", None, &[], &[], 1500);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        at(&r.log, "move lift top"),
        [720, 1440],
        "every: 6 seconds is every 720 steps"
    );
    assert_eq!(
        at(&r.log, "move crusher down"),
        [0, 240, 480, 720, 960, 1200, 1440]
    );
    assert_eq!(
        at(&r.log, "move crusher closed"),
        [120, 360, 600, 840, 1080, 1320]
    );
    assert_eq!(r.tasks, 2, "the every: loop and the crusher loop");
}

#[test]
fn waves_are_lines_of_code_that_wait() {
    let r = run_example("ambush", None, &[(10, "hall.enter")], &[(300, 0.0)], 700);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // waitDestroyed polls every 12 steps from tick 130, so it sees the count
    // reach 0 at tick 310, and the 2 second wait then ends at 550.
    assert_eq!(
        strs(&r.log),
        [
            "[   0] pose hall_exit open",
            "[   0] pose hall_entry open",
            "[   0] move hall_entry open",
            "[  10] move hall_entry closed",
            "[ 130] spawnAll drone",
            "[ 550] say Sector clear, repairs online",
            "[ 550] heal",
            "[ 550] spawnAll hunter",
            "[ 550] move hall_exit open",
        ]
    );
}

#[test]
fn hacking_a_terminal_and_tuning_an_enemy() {
    let expected = [
        "[   0] spawn terminal",
        "[  40] faction sec_drones friendly",
        "[  40] disable sec_turrets",
        "[  40] say Security override: drones are on your side",
    ];
    for level in ["terminal", "terminal2"] {
        let r = run_example(level, None, &[(40, "red_key.use")], &[], 100);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(strs(&r.log), expected, "{level}");
    }
}

#[test]
fn a_test_is_one_more_task_whose_waits_advance_the_game() {
    let r = run_example("switches", Some("vault_test"), &[], &[], 700);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // The stub host does not simulate the vault, so poseNow stays 'closed' and
    // the second assert reports false. What this proves is the timing: three
    // targets at 0.8 s of fire and 0.1 s of waiting each (108 steps), then 2 s.
    assert_eq!(
        strs(&r.log)[4..],
        [
            "[   0] assert true: vault starts closed",
            "[   0] teleport",
            "[   0] aim",
            "[ 108] teleport",
            "[ 108] aim",
            "[ 216] teleport",
            "[ 216] aim",
            "[ 564] assert false: vault opens once every switch is destroyed",
            "[ 564] shot vault-open.png",
        ]
    );
    assert_eq!(r.tasks, 0, "the test task finished, so the run could end");
}
