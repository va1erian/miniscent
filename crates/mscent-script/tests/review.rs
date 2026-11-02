//! Regressions from reviewing the crate: load time on hostile text, collection
//! when nothing is allocating, and `^` at top level.

mod common;
use common::*;
use mscent_script::MAX_LOAD_ERRORS;
use std::time::{Duration, Instant};

/// Fails on a load slow enough to stall the editor or a level start.
fn load_is_quick(src: &str) -> Vec<String> {
    let started = Instant::now();
    let errors = load_errors(&[("level.st", src)]);
    let took = started.elapsed();
    assert!(took < Duration::from_millis(500), "load took {took:?}");
    errors
}

#[test]
fn a_huge_selector_does_not_make_the_missing_period_hint_quadratic() {
    let src = "a: 1 ".repeat((1 << 20) / 5 - 1);
    let errors = load_is_quick(&src);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("nothing understands"), "{errors:?}");
}

#[test]
fn the_missing_period_hint_still_works_on_real_sizes() {
    let src = "x := mover: 'lift'.\nx moveTo: 'open' in: 1\nsay: 'a'.";
    let errors = load_is_quick(src);
    assert!(errors[0].contains("'.' is probably missing"), "{errors:?}");
}

#[test]
fn no_hints_are_worked_out_past_the_error_cap() {
    let src: String = (0..80_000).map(|i| format!("z{i}. ")).collect();
    let errors = load_is_quick(&src);
    assert_eq!(errors.len(), MAX_LOAD_ERRORS);
}

#[test]
fn a_full_heap_with_nothing_allocating_is_not_collected_every_step() {
    // About three quarters of the heap, live, in tens of thousands of objects.
    let src = "a := {}. fork: [ 130 timesRepeat: [ 500 timesRepeat: [ a := {a} ]. wait: 0 ] ].";
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, 140, |_, _, _| {});
    assert_eq!(vm.take_errors(), []);
    assert!(vm.heap().live_objects() > 50_000);
    let live = vm.heap().bytes();
    let started = Instant::now();
    for _ in 0..1000 {
        vm.step(&mut host);
    }
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(250),
        "idle steps took {took:?}"
    );
    assert_eq!(vm.heap().bytes(), live, "nothing it holds went away");
}

#[test]
fn garbage_is_freed_after_running_out_even_when_live_data_fills_the_heap() {
    // One handler runs out of room in a step. The next must find it again.
    let src = "a := {}. s := 'x'. 11 timesRepeat: [ s := s , s ].
        fork: [ 92 timesRepeat: [ 500 timesRepeat: [ a := {a} ]. wait: 0 ] ].
        hold: [ 1100 timesRepeat: [ s , 'y' ] ].
        hold: [ 20 timesRepeat: [ s , 'y' ]. say: 'room again' ].";
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, 104, |t, vm, host| {
        if t == 100 {
            vm.fire(host.handlers[0].1, &[]);
        }
        if t == 102 {
            vm.fire(host.handlers[1].1, &[]);
        }
    });
    let errors = vm.take_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].msg.contains("out of script memory"), "{errors:?}");
    assert_eq!(says(&host.log), ["say room again"]);
}

#[test]
fn caret_at_top_level_ends_only_that_handler() {
    let src = "n := 0.
        hold: [ n := n + 1. n > 1 ifTrue: [ ^ nil ]. say: 'first time' ].
        say: 'top'.";
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, 4, |t, vm, host| {
        if t <= 2 {
            fire(vm, host, "held");
        }
    });
    assert_eq!(vm.take_errors(), []);
    assert_eq!(says(&host.log), ["say top", "say first time"]);
    assert_eq!(vm.task_count(), 0);
}

#[test]
fn caret_in_a_top_level_statement_ends_that_source_only() {
    let loaded = load(&[
        ("a.st", "say: 'a'. ^ nil. say: 'no'."),
        ("b.st", "say: 'b'."),
    ]);
    let (mut vm, mut host) = (loaded.unwrap().vm, Stub::default());
    drive(&mut vm, &mut host, 1, |_, _, _| {});
    assert_eq!(vm.take_errors(), []);
    assert_eq!(says(&host.log), ["say a", "say b"]);
}

#[test]
fn caret_in_a_function_still_returns_from_the_function() {
    let (log, errors) = run_plain("define f [ ^ 1. say: 'no' ] say: f.", 1);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(says(&log), ["say 1"]);
}
