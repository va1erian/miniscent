//! Hostile scripts: every one ends in an error or a bounded run, never a
//! panic or a hang.

mod common;
use common::*;
use mscent_script::{MAX_TASKS, OPS_PER_RUN, RUNS_PER_STEP};
use std::time::{Duration, Instant};

/// Loads and runs a script for a few steps and returns every error, load or
/// runtime, as text.
fn outcome(src: &str) -> Vec<String> {
    let started = Instant::now();
    let errors = match level(src) {
        Err(es) => es.iter().map(ToString::to_string).collect(),
        Ok(loaded) => {
            let (mut vm, mut host) = (loaded.vm, Stub::default());
            drive(&mut vm, &mut host, 5, |_, _, _| {});
            assert!(vm.task_count() <= MAX_TASKS);
            vm.take_errors().iter().map(ToString::to_string).collect()
        }
    };
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    errors
}

#[test]
fn each_hostile_script_gives_the_error_it_should() {
    let many_items = vec!["1"; 1000].join(". ");
    let stack_bomb = format!("define f [ ^ {{{many_items}. f}} ]\nf.");
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "infinite loop",
            "[true] whileTrue: [1 + 1].".into(),
            "over its budget of 20000 operations",
        ),
        (
            "string bomb",
            "s := 'xxxxxxxx'. [true] whileTrue: [s := s , s].".into(),
            "string longer than 4096",
        ),
        (
            "recursion",
            "define f: x [ f: x + 1 ] f: 0.".into(),
            "calls nested more than 128 deep",
        ),
        (
            "block recursion",
            "b := nil. b := [b value]. b value.".into(),
            "calls nested more than 128 deep",
        ),
        (
            "mutual recursion",
            "define a [ b ] define b [ a ] a.".into(),
            "calls nested more than 128 deep",
        ),
        (
            "value stack bomb",
            stack_bomb,
            "value stack deeper than 4096",
        ),
        ("wrong type", "'a' + 1.".into(), "'a' does not understand +"),
        (
            "nil block",
            "nil value.".into(),
            "nil does not understand value",
        ),
        ("divide by zero", "1 / 0.".into(), "not finite"),
        ("bad index", "{1} at: 5.".into(), "index 5 is outside 1..1"),
        (
            "not a boolean",
            "3 ifTrue: [1].".into(),
            "expected true or false, got 3",
        ),
        (
            "while on a number",
            "[3] whileTrue: [1].".into(),
            "expected true or false, got 3",
        ),
        (
            "return after return",
            "define mk [ ^ [^ 1] ] mk value.".into(),
            "already returned",
        ),
        (
            "typo in an unfired handler",
            "(zone: 'hall') onEnter: [ (mover: 'vault_door') moveToo: 'open' in: 1 ].".into(),
            "did you mean 'moveTo:in:'",
        ),
        (
            "undefined variable",
            "say: nosuchthing.".into(),
            "not a variable or a function",
        ),
        (
            "missing bracket",
            "(zone: 'hall') onEnter: [ say: 'x' .".into(),
            "never closed",
        ),
        (
            "stray bracket",
            "say: 'x'. ].".into(),
            "expected a value, found ']'",
        ),
        (
            "stray define",
            "[ define f [ 1 ] ].".into(),
            "expected a value, found 'define'",
        ),
        (
            "control character",
            "say: 'a'.\u{1}".into(),
            "unexpected character",
        ),
        (
            "non-ascii name",
            "\u{e9} := 1.".into(),
            "unexpected character",
        ),
        ("lone quote", "say: '".into(), "never closed"),
        ("lone comment", "\" never closed".into(), "never closed"),
        (
            "empty define",
            "define [ 1 ]".into(),
            "expected a name after define",
        ),
    ];
    for (what, src, expected) in cases {
        let errors = outcome(&src);
        assert!(!errors.is_empty(), "{what}: no error");
        assert!(
            errors.iter().any(|e| e.contains(expected)),
            "{what}: wanted '{expected}' in {errors:?}"
        );
    }
}

#[test]
fn unicode_and_nul_in_strings_and_comments_are_fine() {
    let (log, errors) = run_plain(
        "\"caf\u{e9} \u{1f600}\" say: 'na\u{ef}ve \u{1f600}' , 'a\u{0}b'.",
        2,
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(log, ["[   0] say na\u{ef}ve \u{1f600}a\u{0}b"]);
}

#[test]
fn a_task_that_waits_zero_forever_costs_one_run_a_step() {
    let (mut vm, mut host) = vm_of("[true] whileTrue: [wait: 0].");
    drive(&mut vm, &mut host, 1000, |_, _, _| {});
    assert!(vm.errors().is_empty());
    assert!(vm.ops() < 20_000, "{} ops in 1000 steps", vm.ops());
    assert_eq!(vm.task_count(), 1);
}

#[test]
fn many_runaway_handlers_cost_at_most_sixteen_budgets_a_step() {
    let (mut vm, mut host) = vm_of("(zone: 'hall') onEnter: [[true] whileTrue: [1 + 1]].");
    vm.start(&mut host);
    let block = host.handlers[0].1;
    for _ in 0..MAX_TASKS {
        assert!(vm.fire(block, &[]).is_some());
    }
    let started = Instant::now();
    let mut previous = vm.ops();
    for t in 1..=4 {
        host.tick = t;
        vm.step(&mut host);
        let spent = vm.ops() - previous;
        previous = vm.ops();
        assert!(
            spent <= u64::from(OPS_PER_RUN) * RUNS_PER_STEP as u64,
            "step {t} ran {spent} ops"
        );
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        vm.errors().len(),
        4 * RUNS_PER_STEP,
        "every run ended in a budget error"
    );
    assert_eq!(vm.task_count(), MAX_TASKS - 4 * RUNS_PER_STEP);
}

#[test]
fn a_handler_that_fails_is_removed_and_the_others_keep_running() {
    let src = "(zone: 'hall') onEnter: [1 / 0]. (zone: 'hall') onExit: [say: 'still here'].";
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, 4, |t, vm, host| {
        if t == 1 {
            fire(vm, host, "hall.enter");
        }
        if t == 3 {
            fire(vm, host, "hall.exit");
        }
    });
    assert_eq!(vm.take_errors().len(), 1);
    assert_eq!(host.log, ["[   3] say still here"]);
}

#[test]
fn a_timer_that_forks_every_step_stops_at_the_task_cap() {
    let (mut vm, mut host) = vm_of("every: 0.001 do: [fork: [wait: 1000]].");
    drive(&mut vm, &mut host, 400, |_, _, _| {});
    let errors = vm.take_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].msg.contains("too many tasks"), "{errors:?}");
    assert!(vm.task_count() <= MAX_TASKS);
}
