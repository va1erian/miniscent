//! Error and warning messages: the right file, the right line, and a hint
//! at the likely mistake.

mod common;
use common::*;
use mscent_script::{Flow, MAX_LOAD_ERRORS, Native, Source, Value, Vm};

fn one_error(sources: &[(&str, &str)]) -> String {
    let errors = load_errors(sources);
    assert_eq!(errors.len(), 1, "{errors:?}");
    errors[0].clone()
}

#[test]
fn a_missing_period_between_keyword_statements_is_named() {
    let e = one_error(&[("level.st", "say: 'c'\nsay: 'd'.")]);
    assert_eq!(
        e,
        "level.st:1: nothing understands 'say:say:', but each of its parts is a known message, so a '.' is probably missing before 'say:' on line 2"
    );
}

#[test]
fn a_missing_period_after_a_longer_statement_names_the_split() {
    let e = one_error(&[(
        "level.st",
        "(mover: 'vault_door') moveTo: 'open' in: 1\n\nsay: 'x'.",
    )]);
    assert!(
        e.contains("moveTo:in:say:") && e.contains("probably missing before 'say:' on line 3"),
        "{e}"
    );
}

#[test]
fn a_missing_period_before_an_assignment_is_a_parse_error_naming_the_line() {
    let e = one_error(&[("level.st", "x := 3\ny := 4.\nsay: x + y.")]);
    assert_eq!(
        e,
        "level.st:2: expected '.' between statements, found 'y' (a '.' is probably missing at the end of line 1)"
    );
}

#[test]
fn an_unknown_selector_with_unknown_parts_is_not_blamed_on_a_period() {
    let e = one_error(&[("level.st", "(mover: 'vault_door') moveToo: 'open' in: 1.")]);
    assert_eq!(
        e,
        "level.st:1: nothing understands 'moveToo:in:' (did you mean 'moveTo:in:'?)"
    );
}

#[test]
fn a_bare_unknown_word_says_it_is_neither_a_variable_nor_a_function() {
    let e = one_error(&[("level.st", "say: nosuchthing.")]);
    assert!(
        e.starts_with("level.st:1: 'nosuchthing' is not a variable or a function"),
        "{e}"
    );
}

#[test]
fn a_level_name_typo_is_a_load_error_with_a_suggestion() {
    let e = one_error(&[(
        "level.st",
        "(zone: 'hall') onEnter: [ mover: 'vault_dor' ].",
    )]);
    assert_eq!(
        e,
        "level.st:1: mover: 'vault_dor': the level has no such name (did you mean 'vault_door'?)"
    );
}

#[test]
fn load_errors_name_their_own_file_and_line() {
    let lib = "define a [ 1 ]\n\ndefine b [ 2 ]\ndefine c [ nosuch ]\n";
    let level = "say: 'ok'.\nsay: 'ok'.\n\n\nsay: alsonosuch.\n";
    let errors = load_errors(&[("lib/x.st", lib), ("level.st", level)]);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].starts_with("lib/x.st:4: 'nosuch'"), "{errors:?}");
    assert!(
        errors[1].starts_with("level.st:5: 'alsonosuch'"),
        "{errors:?}"
    );
}

#[test]
fn parse_errors_in_every_source_are_reported_each_with_its_file() {
    let errors = load_errors(&[
        ("lib.st", "define a [ 1 ].\n\nsay: )"),
        ("level.st", "x := .\n"),
        ("level.test.st", "ok."),
    ]);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0], "lib.st:3: expected a value, found ')'");
    assert_eq!(errors[1], "level.st:1: expected a value, found '.'");
}

#[test]
fn an_unclosed_block_names_the_line_it_opened_on() {
    let e = one_error(&[(
        "level.st",
        "say: 'a'.\n\n(zone: 'hall') onEnter: [ say: 'x' .\n",
    )]);
    assert!(
        e.starts_with("level.st:3: this block is never closed"),
        "{e}"
    );
}

#[test]
fn runtime_errors_in_a_library_function_name_the_library_file_and_line() {
    let lib = "\"a library\"\ndefine boom: x [\n  ^ x + 'a'\n]\n";
    let loaded = load(&[("lib/boom.st", lib), ("level.st", "\n\nsay: (boom: 1).")]).expect("loads");
    let (mut vm, mut host) = (loaded.vm, Stub::default());
    drive(&mut vm, &mut host, 2, |_, _, _| {});
    let errors = vm.take_errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].to_string(),
        "lib/boom.st:3: 1 does not understand +"
    );
    assert!(errors[0].task.is_some());
}

#[test]
fn runtime_errors_in_a_handler_fired_later_name_the_level_line() {
    let src = "(zone: 'hall') onEnter: [\n  say: 'before'.\n  say: 'a' + 1.\n].";
    let (mut vm, mut host) = vm_of(src);
    drive(&mut vm, &mut host, 5, |t, vm, host| {
        if t == 2 {
            fire(vm, host, "hall.enter");
        }
    });
    let errors = vm.take_errors();
    assert_eq!(
        errors[0].to_string(),
        "level.st:3: 'a' does not understand +"
    );
    assert_eq!(host.log, ["[   2] say before"]);
}

#[test]
fn a_test_source_keeps_its_own_file_and_line_numbers() {
    let loaded = load(&[
        ("level.st", "x := 1.\nsay: x.\n"),
        ("level.test.st", "\n\n\nsay: 1 + 'q'."),
    ])
    .expect("loads");
    let (mut vm, mut host) = (loaded.vm, Stub::default());
    drive(&mut vm, &mut host, 2, |_, _, _| {});
    assert_eq!(
        vm.take_errors()[0].to_string(),
        "level.test.st:4: 1 does not understand +"
    );
    assert_eq!(
        host.log,
        ["[   0] say 1"],
        "the level's task was not stopped by the test's failure"
    );
}

#[test]
fn an_error_inside_the_prelude_names_the_prelude() {
    let (_, errors) = run_plain("3 do: [:x | x].", 2);
    assert_eq!(errors, ["<prelude>:2: 3 does not understand size"]);
}

#[test]
fn firing_something_that_is_not_a_block_or_has_the_wrong_arguments_is_an_error() {
    let src = "(zone: 'hall') onEnter: [:who |\n  say: who].";
    let (mut vm, mut host) = vm_of(src);
    vm.start(&mut host);
    assert!(vm.fire(Value::Num(1.0), &[]).is_none());
    let block = host.handlers[0].1;
    assert!(vm.fire(block, &[]).is_none());
    let errors: Vec<_> = vm.take_errors().iter().map(ToString::to_string).collect();
    assert_eq!(
        errors,
        [
            "<host>:0: 1 does not understand value",
            "level.st:1: this block takes 1 arguments, but got 0"
        ]
    );
    assert!(vm.fire(block, &[Value::Num(7.0)]).is_some());
    vm.step(&mut host);
    assert_eq!(says(&host.log), ["say 7"]);
}

#[test]
fn a_variable_assigned_and_never_read_is_a_warning_not_an_error() {
    let src = "timerRunning := false.\n(zone: 'hall') onEnter: [\n  timerRuning := true ].\nsay: timerRunning.";
    let loaded = level(src).expect("a typo in an assignment still loads");
    let warnings: Vec<_> = loaded.warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        warnings,
        ["level.st:3: variable 'timerRuning' is assigned but never read (a typo in the name?)"]
    );
}

#[test]
fn warnings_name_the_file_and_skip_variables_that_are_read_anywhere() {
    let lib = "shared := 1.\nunused := 2.\ndefine f [ ^ shared ]\n";
    let level = "n := 0.\n[:x | local := x] value: 1.\nsay: f.\nsay: n.";
    let loaded = load(&[("lib.st", lib), ("level.st", level)]).expect("loads");
    let warnings: Vec<_> = loaded.warnings.iter().map(|w| w.to_string()).collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0].starts_with("lib.st:2: variable 'unused'"),
        "{warnings:?}"
    );
    assert!(
        warnings[1].starts_with("level.st:2: variable 'local'"),
        "{warnings:?}"
    );
}

#[test]
fn clean_scripts_have_no_warnings() {
    for src in [
        "x := 1. say: x.",
        "define f: a [ b := a. ^ b ] say: (f: 1).",
        "n := 0. every: 1 do: [n := n + 1].",
    ] {
        assert!(level(src).expect("loads").warnings.is_empty(), "{src}");
    }
}

#[test]
fn a_name_defined_twice_or_colliding_with_the_language_or_a_native_is_rejected() {
    let dup = load_errors(&[
        ("lib.st", "define f [ 1 ]"),
        ("level.st", "\ndefine f [ 2 ]"),
    ]);
    assert_eq!(dup, ["level.st:2: 'f' is already defined at lib.st:1"]);
    let prelude = one_error(&[("level.st", "define arr do: b [ nil ]")]);
    assert!(
        prelude.contains("'do:' is already defined at <prelude>:"),
        "{prelude}"
    );
    let language = one_error(&[("level.st", "define x ifTrue: b [ nil ]")]);
    assert!(
        language.contains("a message the language provides"),
        "{language}"
    );
    let native = one_error(&[("level.st", "define say: s [ nil ]")]);
    assert!(native.contains("is a game function"), "{native}");
}

#[test]
fn a_native_with_a_language_name_or_twice_in_the_table_is_rejected() {
    let f: Native<Stub> = |_, _, _, _| Ok(Flow::Val(Value::Nil));
    let sources = [Source {
        name: "level.st",
        text: "nil.",
    }];
    let bad = Vm::<Stub>::load(&sources, &[("wait:", f), ("a", f), ("a", f)], &[])
        .err()
        .expect("rejected");
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(
        bad[0].msg.contains("language message") && bad[1].msg.contains("twice"),
        "{bad:?}"
    );
}

#[test]
fn receivers_must_match_how_a_function_was_defined() {
    let plain = one_error(&[("level.st", "define f: a [ ^ a ]\nsay: (3 f: 4).")]);
    assert!(
        plain.starts_with("level.st:2: 'f:' is a plain function"),
        "{plain}"
    );
    let method = one_error(&[("level.st", "define x f [ ^ x ]\nsay: f.")]);
    assert!(method.starts_with("level.st:2: 'f' is defined"), "{method}");
}

#[test]
fn inline_messages_need_literal_blocks() {
    let e = one_error(&[("level.st", "b := [1]. true ifTrue: b.")]);
    assert!(e.contains("'ifTrue:' needs literal blocks"), "{e}");
}

#[test]
fn values_cannot_be_assigned_or_used_as_names() {
    for src in [
        "true := 3.",
        "define f: nil [ nil ]",
        "[:false | 1] value: 1.",
    ] {
        let e = one_error(&[("level.st", src)]);
        assert!(e.contains("is a value, so it cannot be a"), "{e}");
    }
    assert!(one_error(&[("level.st", "[:a :a | 1] value: 1 value: 2.")]).contains("appears twice"));
}

#[test]
fn a_flood_of_errors_is_cut_off() {
    let errors = load_errors(&[("level.st", &"nosuch. ".repeat(5000))]);
    assert_eq!(errors.len(), MAX_LOAD_ERRORS);
}
