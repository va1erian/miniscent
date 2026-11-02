//! VM state: stale references, cloning, fairness, garbage collection, replay.

mod common;
use common::*;
use mscent_script::{RUNS_PER_STEP, Vm};

fn assert_send_and_clone<T: Send + Clone>() {}

#[test]
fn the_vm_is_send_and_clone() {
    assert_send_and_clone::<Vm<Stub>>();
}

// ---- Item 5: generation-checked references

#[test]
fn a_stale_reference_reads_nothing_even_after_its_slot_is_reused() {
    let (mut vm, mut host) = vm_of("stash: 'ab' , 'cd'. keep: 'ef' , 'gh'.");
    vm.start(&mut host);
    let (stale, kept) = (host.unrooted[0], host.rooted[0]);
    assert_eq!(vm.heap().str(stale), Some("abcd"));
    vm.collect(&host);
    assert_eq!(
        vm.heap().str(stale),
        None,
        "the unrooted string was collected"
    );
    assert_eq!(vm.heap().str(kept), Some("efgh"), "the rooted one was not");
    for i in 0..50 {
        let fresh = vm.heap_mut().new_str(format!("new{i}")).expect("allocates");
        assert_eq!(
            vm.heap().str(fresh).map(str::to_string),
            Some(format!("new{i}"))
        );
        assert_eq!(
            vm.heap().str(stale),
            None,
            "the old reference must not alias slot reuse"
        );
    }
}

#[test]
fn firing_a_stale_block_is_an_error_not_whatever_lives_there_now() {
    let (mut vm, mut host) = vm_of("stash: [say: 'old'].");
    vm.start(&mut host);
    let stale = host.unrooted[0];
    vm.collect(&host);
    // Fill the freed slots with other objects, including blocks.
    for _ in 0..20 {
        vm.heap_mut().new_array(vec![]).expect("allocates");
    }
    assert!(vm.fire(stale, &[]).is_none());
    let errors = vm.take_errors();
    assert_eq!(
        errors[0].to_string(),
        "<host>:0: a stale reference does not understand value"
    );
    assert!(host.log.is_empty());
}

// ---- Item 9: Clone

fn clone_script() -> &'static str {
    "n := 0.
     every: 1 do: [n := n + 1. say: n].
     fork: [wait: 5. say: 'five'. wait: 5. say: 'ten'].
     (zone: 'hall') onEnter: [say: 'enter ' , n]."
}

fn play(vm: &mut Vm<Stub>, host: &mut Stub, from: u64, to: u64) {
    for t in from..=to {
        host.tick = t;
        if t == 200 || t == 500 {
            fire(vm, host, "hall.enter");
        }
        vm.step(host);
    }
}

#[test]
fn a_clone_taken_mid_wait_runs_on_exactly_like_the_original() {
    let (mut vm, mut host) = vm_of(clone_script());
    vm.start(&mut host);
    play(&mut vm, &mut host, 1, 130);
    assert_eq!(
        vm.task_count(),
        2,
        "the every: loop and the fork are both mid-wait"
    );
    let (mut vm2, mut host2) = (vm.clone(), host.clone());
    // Run the clone first: if state were shared, the original would continue
    // from where the clone stopped instead of from tick 130.
    play(&mut vm2, &mut host2, 131, 1400);
    play(&mut vm, &mut host, 131, 1400);
    assert_eq!(host.log, host2.log);
    assert_eq!(vm.errors(), vm2.errors());
    assert_eq!(
        (vm.ops(), vm.tick(), vm.task_count()),
        (vm2.ops(), vm2.tick(), vm2.task_count())
    );
    assert_eq!(vm.heap().bytes(), vm2.heap().bytes());
    let log = says(&host.log);
    assert!(
        log.contains(&"say five".to_string()) && log.contains(&"say ten".to_string()),
        "{log:?}"
    );
    assert!(log.contains(&"say enter 1".to_string()), "{log:?}");
}

#[test]
fn stepping_a_clone_leaves_the_original_untouched() {
    let (mut vm, mut host) = vm_of(clone_script());
    vm.start(&mut host);
    play(&mut vm, &mut host, 1, 130);
    let (before_log, before_ops, before_bytes) = (host.log.clone(), vm.ops(), vm.heap().bytes());
    let (mut vm2, mut host2) = (vm.clone(), host.clone());
    play(&mut vm2, &mut host2, 131, 600);
    assert_eq!(
        (host.log.clone(), vm.ops(), vm.heap().bytes()),
        (before_log, before_ops, before_bytes)
    );
    assert_eq!(vm.tick(), 130);
}

// ---- Item 10: fairness

const SPINNERS: &str = "define spin: n [ fork: [ [true] whileTrue: [ note: n. wait: 0.001 ] ] ]
    i := 1. [i <= 40] whileTrue: [ spin: i. i := i + 1 ].";

fn spin_counts() -> Vec<u32> {
    let (mut vm, mut host) = vm_of(SPINNERS);
    drive(&mut vm, &mut host, 200, |_, _, _| {});
    assert!(vm.errors().is_empty(), "{:?}", vm.errors());
    host.counts
}

#[test]
fn forty_busy_tasks_share_sixteen_runs_a_step_evenly() {
    let counts = spin_counts();
    let spinners = &counts[1..=40];
    // 201 passes (the start and 200 steps) of 16 runs, and the first run is the
    // task that forks them, which notes nothing.
    assert_eq!(spinners.iter().sum::<u32>(), 16 * 201 - 1);
    let (lo, hi) = (
        spinners.iter().min().copied().unwrap_or(0),
        spinners.iter().max().copied().unwrap_or(0),
    );
    assert!(
        lo >= 79 && hi - lo <= 1,
        "no task starves: {lo}..{hi} of {spinners:?}"
    );
    assert_eq!(RUNS_PER_STEP, 16);
}

#[test]
fn the_deferred_order_is_deterministic() {
    assert_eq!(spin_counts(), spin_counts());
}

// ---- Garbage collection

#[test]
fn a_long_run_stays_at_a_bounded_live_object_count() {
    let src = "every: 0.01 do: [t := {1. 2. 3}. u := 'a' , 'b'. w := [t size]. w value].";
    let (mut vm, mut host) = vm_of(src);
    vm.start(&mut host);
    let (mut max_objects, mut max_bytes) = (0, 0);
    for t in 1..=20_000 {
        host.tick = t;
        vm.step(&mut host);
        if t % 50 == 0 {
            max_objects = max_objects.max(vm.heap().live_objects());
            max_bytes = max_bytes.max(vm.heap().bytes());
        }
    }
    assert!(
        vm.ops() > 150_000,
        "{} ops: the run really did churn",
        vm.ops()
    );
    assert!(vm.errors().is_empty(), "{:?}", vm.errors());
    // Garbage builds up until the heap reaches the 256 KiB collection
    // threshold, then drops; it never grows past that however long it runs.
    assert!(max_objects < 10_000, "{max_objects} live objects");
    assert!(max_bytes < 300 << 10, "{max_bytes} bytes");
    vm.collect(&host);
    assert!(
        vm.heap().live_objects() < 100,
        "{} left after a collection",
        vm.heap().live_objects()
    );
}

#[test]
fn a_block_the_host_holds_survives_collection_with_its_variables() {
    let (mut vm, mut host) =
        vm_of("n := 0. (zone: 'hall') onEnter: [n := n + 1. say: 'n is ' , n].");
    vm.start(&mut host);
    for round in 0..3 {
        for _ in 0..20 {
            vm.heap_mut().new_str("garbage".into()).expect("allocates");
        }
        vm.collect(&host);
        fire(&mut vm, &host, "hall.enter");
        host.tick += 1;
        vm.step(&mut host);
        assert_eq!(
            says(&host.log).last().cloned(),
            Some(format!("say n is {}", round + 1))
        );
    }
    assert!(vm.errors().is_empty());
}

#[test]
fn a_block_held_only_by_a_waiting_task_survives_collection() {
    let (mut vm, mut host) = vm_of("fork: [b := [say: 'later']. wait: 1. b value].");
    vm.start(&mut host);
    for t in 1..=130 {
        host.tick = t;
        vm.collect(&host);
        vm.step(&mut host);
    }
    assert_eq!(host.log, ["[ 120] say later"]);
    assert!(vm.errors().is_empty());
}

// ---- Replay

fn replay_script() -> String {
    // The ambush, a fork that fails midway, and a closure counter.
    format!(
        "{}\nfork: [3 timesRepeat: [wait: 0.5]. 1 / 0].\nc := 0. every: 0.7 do: [c := c + 1].",
        script("ambush")
    )
}

fn replay() -> (Vec<String>, Vec<String>, u64, usize, usize) {
    let (wait, src) = (script("lib_wait"), replay_script());
    let loaded = load(&[("lib.st", &wait), ("level.st", &src)]).expect("loads");
    let (mut vm, mut host) = (
        loaded.vm,
        Stub {
            alive: 2.0,
            ..Stub::default()
        },
    );
    drive(&mut vm, &mut host, 900, |t, vm, host| {
        if t == 10 || t == 12 {
            fire(vm, host, "hall.enter");
        }
        if t == 300 {
            host.alive = 0.0;
        }
    });
    let errors = vm.take_errors().iter().map(ToString::to_string).collect();
    (
        host.log,
        errors,
        vm.ops(),
        vm.heap().bytes(),
        vm.task_count(),
    )
}

#[test]
fn the_same_script_and_events_twice_give_identical_logs_errors_and_op_counts() {
    let (a, b) = (replay(), replay());
    assert_eq!(a, b);
    assert!(
        a.0.len() > 8 && a.2 > 500,
        "the run did something: {} lines, {} ops",
        a.0.len(),
        a.2
    );
    assert_eq!(a.1.len(), 1, "{:?}", a.1);
    assert!(
        a.1[0].starts_with("level.st:") && a.1[0].contains("not finite"),
        "{:?}",
        a.1
    );
}
