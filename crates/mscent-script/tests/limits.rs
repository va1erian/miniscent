//! Every limit is a load error or a script error, never a wrapped index, a
//! stack overflow or a panic. Each test here fails without its fix.

mod common;
use common::*;
use mscent_script::{
    Flow, MAX_ARGS, MAX_HEAP_BYTES, MAX_LITERAL_ITEMS, MAX_SLOTS, MAX_SOURCE, MAX_TASKS, Native,
    Value,
};

fn errors_of(src: String) -> Vec<String> {
    load_errors(&[("level.st", &src)])
}

fn has(errors: &[String], needle: &str) -> bool {
    errors.iter().any(|e| e.contains(needle))
}

// ---- Item 1: long chains and deep nesting must not overflow the Rust stack

/// The sources below are close to the 1 MiB file limit, and are loaded on a
/// thread with a quarter of a normal test stack.
#[test]
fn a_binary_chain_near_the_file_limit_loads_and_drops_safely() {
    let (loaded_ok, len) = on_small_stack(|| {
        let src = format!("x := 1{}.\nsay: x.", " + 1".repeat(MAX_SOURCE / 4 - 16));
        let len = src.len();
        let loaded = level(&src).expect("a long chain is not an error");
        let (mut vm, mut host) = (loaded.vm, Stub::default());
        drive(&mut vm, &mut host, 2, |_, _, _| {});
        let errors = vm.take_errors();
        assert!(errors[0].msg.contains("over its budget"), "{errors:?}");
        (true, len)
    });
    assert!(
        loaded_ok && len > MAX_SOURCE - 100 && len <= MAX_SOURCE,
        "{len}"
    );
}

#[test]
fn a_unary_chain_near_the_file_limit_loads_and_drops_safely() {
    let len = on_small_stack(|| {
        let src = format!(
            "define x b [ ^ x ]\nx := 1. x{}.",
            " b".repeat(MAX_SOURCE / 2 - 64)
        );
        let len = src.len();
        let loaded = level(&src).expect("a long chain is not an error");
        drop(loaded);
        len
    });
    assert!(len > MAX_SOURCE - 200 && len <= MAX_SOURCE, "{len}");
}

#[test]
fn a_keyword_chain_is_one_message_and_hits_the_argument_limit() {
    let parts = MAX_ARGS + 45;
    let name: &'static str = Box::leak("k:".repeat(parts).into_boxed_str());
    let table: Vec<(&str, Native<Stub>)> = vec![(name, |_, _, _, _| Ok(Flow::Val(Value::Nil)))];
    let src = format!("nil {}.", "k: 1 ".repeat(parts));
    let errors = load_natives(&table, &src)
        .err()
        .expect("too many arguments");
    assert!(
        errors[0].msg.contains("arguments; the limit is 255"),
        "{errors:?}"
    );
}

#[test]
fn nesting_is_capped_and_a_deep_but_legal_script_works() {
    on_small_stack(|| {
        let ok = format!("say: {}1{}.", "(".repeat(60), ")".repeat(60));
        assert_eq!(run_plain(&ok, 2).0, ["[   0] say 1"]);
        let bad = format!("x := {}1{}.", "(".repeat(100_000), ")".repeat(100_000));
        assert!(has(
            &load_errors(&[("level.st", &bad)]),
            "nested more than 64 deep"
        ));
        let blocks = format!("x := {}1{}.", "[".repeat(100_000), "]".repeat(100_000));
        assert!(has(
            &load_errors(&[("level.st", &blocks)]),
            "nested more than 64 deep"
        ));
        let arrays = format!("x := {}1{}.", "{".repeat(100_000), "}".repeat(100_000));
        assert!(has(
            &load_errors(&[("level.st", &arrays)]),
            "nested more than 64 deep"
        ));
        let assigns = format!("{}1.", "x := ".repeat(100_000));
        assert!(has(
            &load_errors(&[("level.st", &assigns)]),
            "nested more than 64 deep"
        ));
    });
}

#[test]
fn deep_blocks_and_calls_that_are_legal_run_in_a_small_stack() {
    on_small_stack(|| {
        let deep = format!(
            "define f: n [ ^ n < 100 ifTrue: [f: n + 1] ifFalse: [n] ]\nsay: (f: 0).{}",
            ""
        );
        let (log, errors) = run_plain(&deep, 2);
        assert!(
            errors.is_empty() && log == ["[   0] say 100"],
            "{log:?} {errors:?}"
        );
    });
}

// ---- Item 2: checked conversions in the compiler

#[test]
fn too_many_distinct_numbers_is_a_load_error_not_a_wrapped_constant() {
    // The prelude uses three of the 65,535 constants: 0, 1 and 0.1.
    let fits = (1000..1000 + 65_532)
        .map(|n| format!("{n}."))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(level(&fits).is_ok(), "65,535 constants fit");
    let over = (1000..1000 + 65_533)
        .map(|n| format!("{n}."))
        .collect::<Vec<_>>()
        .join(" ");
    let errors = errors_of(over);
    assert!(
        has(
            &errors,
            "too many distinct numbers and strings: the limit is 65535"
        ),
        "{errors:?}"
    );
    assert!(errors[0].starts_with("level.st:1:"), "{errors:?}");
}

#[test]
fn repeated_literals_share_one_constant() {
    let src = format!("x := 7.{}", " x := 7. x := 'a'. x := 'a'.".repeat(30_000));
    assert!(level(&src).is_ok());
}

#[test]
fn too_many_distinct_selectors_is_a_load_error() {
    let names: Vec<&'static str> = (0..65_600)
        .map(|i| &*Box::leak(format!("n{i}").into_boxed_str()))
        .collect();
    let table: Vec<(&str, Native<Stub>)> = names
        .iter()
        .map(|&n| (n, (|_, _, _, _| Ok(Flow::Val(Value::Nil))) as Native<Stub>))
        .collect();
    let src = names
        .iter()
        .map(|n| format!("{n}."))
        .collect::<Vec<_>>()
        .join(" ");
    let errors = load_natives(&table, &src)
        .err()
        .expect("too many selectors");
    assert!(
        has(
            &errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "too many distinct message names"
        ),
        "{errors:?}"
    );
}

#[test]
fn too_many_blocks_is_a_load_error() {
    let errors = errors_of("[]. ".repeat(70_000));
    assert!(
        has(&errors, "too many functions and blocks: the limit is 65535"),
        "{errors:?}"
    );
}

#[test]
fn array_literals_are_capped() {
    let items = |n: usize| format!("a := {{{}}}. say: a size.", vec!["1"; n].join(". "));
    assert_eq!(
        run_plain(&items(MAX_LITERAL_ITEMS), 2).0,
        [format!("[   0] say {MAX_LITERAL_ITEMS}")]
    );
    assert!(has(
        &errors_of(items(MAX_LITERAL_ITEMS + 1)),
        "array literal has 1025 items"
    ));
    assert!(
        has(&errors_of(items(70_000)), "array literal has 70000 items"),
        "no wrap at 65,536"
    );
}

#[test]
fn too_many_variables_in_one_scope_is_a_load_error() {
    let vars = |n: usize| {
        (0..n)
            .map(|i| format!("v{i} := {i}. say: v{i}."))
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert!(level(&vars(MAX_SLOTS)).is_ok());
    let top = errors_of(vars(MAX_SLOTS + 1));
    assert!(
        has(&top, "too many variables in one scope: the limit is 1024"),
        "{top:?}"
    );
    let in_function = errors_of(format!("define f [ {} ]", vars(MAX_SLOTS + 1)));
    assert!(
        has(&in_function, "too many variables in one scope"),
        "{in_function:?}"
    );
    let compact = (0..70_000).map(|i| format!("a{i}:=1.")).collect::<String>();
    let in_block = errors_of(format!("[ {compact} ] value."));
    assert!(
        has(&in_block, "too many variables in one scope"),
        "no wrap at 65,536"
    );
}

#[test]
fn too_many_parameters_is_a_load_error() {
    let params = (0..300)
        .map(|i| format!("k{i}: p{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let errors = errors_of(format!("define {params} [ ^ p0 ]"));
    assert!(
        has(&errors, "too many parameters: the limit is 255"),
        "{errors:?}"
    );
}

#[test]
fn a_function_too_long_for_its_jump_targets_cannot_wrap() {
    // Jump targets are u32 and a source is at most 1 MiB, so the checked
    // conversion is unreachable; a long, branchy function must still work.
    let body = "1 < 2 ifTrue: [x := 1] ifFalse: [x := 2]. ".repeat(20_000);
    let loaded = level(&format!("x := 0. {body} say: x.")).expect("loads");
    let (mut vm, mut host) = (loaded.vm, Stub::default());
    drive(&mut vm, &mut host, 2, |_, _, _| {});
    // 20,000 branches are far over the per-run budget, so it stops, cleanly.
    assert!(vm.take_errors()[0].msg.contains("over its budget"));
}

// ---- Item 3: task cap

#[test]
fn a_fork_bomb_stops_at_the_task_cap() {
    let (mut vm, mut host) = vm_of("[true] whileTrue: [fork: [wait: 100]].");
    drive(&mut vm, &mut host, 3, |_, _, _| {});
    let errors = vm.take_errors();
    assert!(
        errors[0].msg.contains("too many tasks: the limit is 128"),
        "{errors:?}"
    );
    assert!(vm.task_count() <= MAX_TASKS, "{} tasks", vm.task_count());
    assert_eq!(
        vm.task_count(),
        MAX_TASKS - 1,
        "the failed task is gone; its 127 children wait"
    );
}

#[test]
fn firing_a_handler_at_the_cap_is_an_error_not_a_new_task() {
    let (mut vm, mut host) = vm_of("(zone: 'hall') onEnter: [wait: 100].");
    vm.start(&mut host);
    let block = host.handlers[0].1;
    let ids: Vec<_> = (0..MAX_TASKS + 5).map(|_| vm.fire(block, &[])).collect();
    assert_eq!(ids.iter().flatten().count(), MAX_TASKS);
    assert_eq!(vm.task_count(), MAX_TASKS);
    let errors = vm.take_errors();
    assert_eq!(errors.len(), 5);
    assert!(
        errors[0]
            .to_string()
            .starts_with("level.st:1: too many tasks"),
        "{}",
        errors[0]
    );
}

// ---- Item 4: heap counted in bytes

#[test]
fn the_heap_limit_counts_bytes_not_objects() {
    // 500 strings of 2 KB per inner array is 1 MB, and the limit is 16 MiB:
    // the old count of 200,000 objects would allow 400 MB of this.
    let src = "s := 'x'. 11 timesRepeat: [s := s , s]. outer := {}.
        [true] whileTrue: [inner := {}. 500 timesRepeat: [inner add: s , 'y']. outer add: inner. wait: 0.01].";
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, 400, |_, _, _| {});
    let errors = vm.take_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0]
            .msg
            .contains("out of script memory (limit 16 MiB)"),
        "{errors:?}"
    );
    assert!(
        vm.heap().bytes() <= MAX_HEAP_BYTES,
        "{} bytes",
        vm.heap().bytes()
    );
    assert!(
        vm.heap().bytes() > MAX_HEAP_BYTES / 2,
        "it really did fill the heap"
    );
    assert!(
        vm.heap().live_objects() < 20_000,
        "an object count would not have stopped this"
    );
}

#[test]
fn heap_bytes_fall_after_a_collection() {
    let (mut vm, mut host) =
        vm_of("s := 'x'. 11 timesRepeat: [s := s , s]. 200 timesRepeat: [t := s , 'y'].");
    vm.start(&mut host);
    let before = vm.heap().bytes();
    assert!(before > 200 * 2048, "{before}");
    vm.collect(&host);
    assert!(
        vm.heap().bytes() < before / 10,
        "{} of {before}",
        vm.heap().bytes()
    );
}

#[test]
fn string_and_array_length_limits_are_script_errors() {
    let (_, errors) = run_plain("s := 'x'. [true] whileTrue: [s := s , s].", 2);
    assert!(
        errors[0].contains("string longer than 4096 bytes"),
        "{errors:?}"
    );
    // Each add: costs a few operations, so fill the array over several runs.
    let (_, errors) = run_plain("a := {}. [true] whileTrue: [a add: 1. wait: 0.001].", 5000);
    assert!(
        errors[0].contains("array longer than 4096 items"),
        "{errors:?}"
    );
    assert!(has(
        &errors_of(format!("say: '{}'.", "x".repeat(5000))),
        "string literal is 5000 bytes; the limit is 4096"
    ));
}

// ---- Item 11: no panics or overflow on any number

#[test]
fn huge_waits_and_bad_numbers_cannot_overflow_the_clock() {
    let huge = "9".repeat(30);
    let (mut vm, mut host) = vm_of(&format!("say: 'a'. wait: {huge}. say: 'never'."));
    drive(&mut vm, &mut host, 50, |_, _, _| {});
    assert_eq!(host.log, ["[   0] say a"]);
    assert_eq!(
        vm.task_count(),
        1,
        "parked for a very long time, not wrapped to now"
    );
    for bad in ["wait: 0 - 1.", "wait: nil.", "wait: 'a'.", "waitnan."] {
        let (_, errors) = run_plain(bad, 2);
        assert_eq!(errors.len(), 1, "{bad}: {errors:?}");
    }
}

#[test]
fn arithmetic_and_indexing_on_extreme_numbers_are_errors() {
    let huge = "9".repeat(30);
    for src in [
        format!("x := {huge}. say: x * x."),
        "say: 1 / 0.".to_string(),
        "say: 0 / 0.".to_string(),
        format!("say: ({{1}} at: {huge})."),
        "say: ({1} at: 0).".to_string(),
        "say: ({1} at: 0 - 1).".to_string(),
        "say: ({1} at: 1.5).".to_string(),
        "{1} at: 2 put: 1.".to_string(),
    ] {
        let (_, errors) = run_plain(&src, 2);
        assert_eq!(errors.len(), 1, "{src}: {errors:?}");
    }
}

#[test]
fn huge_literals_are_errors() {
    let digits = format!("x := {}.", "9".repeat(MAX_SOURCE - 20));
    assert!(has(&errors_of(digits), "number is too large"));
    let long_string = format!("x := '{}'.", "a".repeat(MAX_SOURCE - 20));
    assert!(has(&errors_of(long_string), "string literal is"));
    let over = format!("x := '{}'.", "a".repeat(MAX_SOURCE));
    assert!(has(&errors_of(over), "over the 1048576 byte limit"));
    let ident = format!("say: {}.", "a".repeat(MAX_SOURCE - 20));
    let errors = errors_of(ident);
    assert!(
        errors[0].len() < 300,
        "an identifier is cut down in a message: {}",
        errors[0].len()
    );
}
