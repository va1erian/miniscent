//! One test per construct in "The language" section of docs/scripting.md.

mod common;
use common::*;

fn out(src: &str) -> Vec<String> {
    let (log, errors) = run_plain(src, 400);
    assert!(errors.is_empty(), "runtime errors: {errors:?}");
    says(&log)
}

fn run_error(src: &str) -> String {
    let (_, errors) = run_plain(src, 5);
    assert_eq!(errors.len(), 1, "expected one error, got {errors:?}");
    errors[0].clone()
}

#[test]
fn assignment_declares_on_first_use_and_then_updates() {
    assert_eq!(
        out("x := 3. say: x. x := x + 1. say: x."),
        ["say 3", "say 4"]
    );
}

#[test]
fn keyword_message_goes_to_its_receiver() {
    let log = out("(mover: 'vault_door') moveTo: 'open' in: 1.5.");
    assert_eq!(log, ["move vault_door open"]);
}

#[test]
fn a_message_with_no_receiver_is_a_global_function() {
    assert_eq!(out("say: 'hello'."), ["say hello"]);
}

#[test]
fn unary_messages_and_bare_words_that_are_not_variables() {
    assert_eq!(out("say: player hp."), ["say 30"]);
    assert_eq!(out("say: player."), ["say <2:999>"]);
}

#[test]
fn binary_operators_run_strictly_left_to_right() {
    assert_eq!(out("say: 2 + 3 * 4."), ["say 20"]);
    assert_eq!(out("say: 2 + (3 * 4)."), ["say 14"]);
    assert_eq!(out("say: 10 - 2 - 3."), ["say 5"]);
    assert_eq!(out("say: 100 / 5 / 2."), ["say 10"]);
}

#[test]
fn precedence_is_unary_then_binary_then_keyword() {
    assert_eq!(out("say: 1 + player hp."), ["say 31"]);
    assert_eq!(out("say: 1 + 2 , 'x'."), ["say 3x"]);
    assert_eq!(
        out("(mover: 'vault_door') moveTo: 1 + 1 in: 2 * 3."),
        ["move vault_door 2"]
    );
}

#[test]
fn comparisons_and_equality() {
    let log = out(
        "say: 1 < 2. say: 2 <= 2. say: 3 > 4. say: 4 >= 5. say: 'a' = 'a'. say: 'a' ~= 'b'. say: 1 = 2.",
    );
    assert_eq!(
        log,
        [
            "say true",
            "say true",
            "say false",
            "say false",
            "say true",
            "say true",
            "say false"
        ]
    );
}

#[test]
fn comma_concatenates_and_converts_to_text() {
    assert_eq!(out("say: 'n = ' , 3."), ["say n = 3"]);
    assert_eq!(out("say: 'a' , nil , true , 0.5."), ["say aniltrue0.5"]);
}

#[test]
fn arrays_are_one_indexed_and_grow() {
    let log = out(
        "a := {1. 2. 3}. say: (a at: 1). say: a size. a add: 4. say: a size. a at: 2 put: 9. say: (a at: 2).",
    );
    assert_eq!(log, ["say 1", "say 3", "say 4", "say 9"]);
    assert_eq!(out("say: {} size."), ["say 0"]);
}

#[test]
fn blocks_take_arguments_through_value() {
    assert_eq!(out("say: ([:x | x + 1] value: 1)."), ["say 2"]);
    assert_eq!(out("say: ([:x :y | x * y] value: 3 value: 4)."), ["say 12"]);
    assert_eq!(out("say: [7] value."), ["say 7"]);
}

#[test]
fn a_block_can_change_outer_variables() {
    let log = out("n := 0. inc := [n := n + 1]. inc value. inc value. say: n.");
    assert_eq!(log, ["say 2"]);
}

#[test]
fn closures_keep_their_own_variables_alive() {
    let src = "define counter [ n := 0. ^ [n := n + 1] ] a := counter. b := counter. a value. a value. say: a value. say: b value.";
    assert_eq!(out(src), ["say 3", "say 1"]);
}

#[test]
fn conditionals_return_values() {
    assert_eq!(
        out("say: (1 < 2 ifTrue: ['yes'] ifFalse: ['no'])."),
        ["say yes"]
    );
    assert_eq!(
        out("say: (1 > 2 ifTrue: ['yes'] ifFalse: ['no'])."),
        ["say no"]
    );
    assert_eq!(out("say: (1 > 2 ifFalse: ['f'] ifTrue: ['t'])."), ["say f"]);
    assert_eq!(
        out("1 < 2 ifTrue: [say: 'a']. 1 > 2 ifFalse: [say: 'b']. 1 > 2 ifTrue: [say: 'c']."),
        ["say a", "say b"]
    );
    assert_eq!(out("say: (1 > 2 ifTrue: [1])."), ["say nil"]);
}

#[test]
fn boolean_messages() {
    assert_eq!(
        out(
            "say: (true and: [false]). say: (false or: [true]). say: true not. say: nil isNil. say: 3 isNil."
        ),
        [
            "say false",
            "say true",
            "say false",
            "say true",
            "say false"
        ]
    );
    // `and:` does not evaluate its block once the answer is known.
    assert_eq!(
        out("false and: [say: 'never']. true or: [say: 'never']."),
        Vec::<String>::new()
    );
}

#[test]
fn loops() {
    assert_eq!(
        out("i := 0. [i < 3] whileTrue: [say: i. i := i + 1]."),
        ["say 0", "say 1", "say 2"]
    );
    assert_eq!(
        out("i := 0. [i >= 2] whileFalse: [say: i. i := i + 1]."),
        ["say 0", "say 1"]
    );
    assert_eq!(
        out("3 timesRepeat: [say: 'x']."),
        ["say x", "say x", "say x"]
    );
    assert_eq!(out("{10. 20} do: [:v | say: v]."), ["say 10", "say 20"]);
}

#[test]
fn define_makes_a_function_that_sees_top_level_variables() {
    let src = "total := 0. define add: a to: b [ total := a + b. ^ total ] say: (add: 2 to: 3). say: total.";
    assert_eq!(out(src), ["say 5", "say 5"]);
}

#[test]
fn define_with_a_receiver_names_it_with_the_first_word() {
    assert_eq!(
        out("define arr first [ ^ arr at: 1 ] say: ({7. 8} first)."),
        ["say 7"]
    );
    assert_eq!(
        out("define a plus: b [ ^ a + b ] say: (3 plus: 4)."),
        ["say 7"]
    );
}

#[test]
fn a_function_without_a_return_gives_its_last_value() {
    assert_eq!(out("define f [ 41 + 1 ] say: f."), ["say 42"]);
}

#[test]
fn caret_returns_from_the_method_even_inside_a_block() {
    let src = "define find: arr [ arr do: [:x | x > 1 ifTrue: [^ x]]. ^ 'none' ] say: (find: {1. 2. 3}). say: (find: {0}).";
    assert_eq!(out(src), ["say 2", "say none"]);
}

#[test]
fn caret_unwinds_nested_calls() {
    let src = "define inner: blk [ blk value. say: 'not reached' ] define outer [ inner: [^ 'out']. ^ 'end' ] say: outer.";
    assert_eq!(out(src), ["say out"]);
}

#[test]
fn caret_after_its_method_returned_is_an_error_with_a_line() {
    let e = run_error("define mk [ ^ [^ 1] ]\nb := mk.\nb value.");
    assert!(
        e.starts_with("level.st:1:") && e.contains("already returned"),
        "{e}"
    );
}

#[test]
fn caret_at_the_top_level_ends_that_task() {
    assert_eq!(out("say: 'a'. ^ 1. say: 'b'."), ["say a"]);
}

#[test]
fn wait_pauses_only_its_task_and_resumes_on_the_right_tick() {
    let (log, errors) = run_plain("say: 'a'. wait: 1. say: 'b'. wait: 0.5. say: 'c'.", 400);
    assert!(errors.is_empty());
    assert_eq!(log, ["[   0] say a", "[ 120] say b", "[ 180] say c"]);
}

#[test]
fn wait_rounds_up_to_whole_steps_and_is_at_least_one() {
    let (log, _) = run_plain("wait: 0. say: 'a'. wait: 0.01. say: 'b'.", 20);
    assert_eq!(log, ["[   1] say a", "[   3] say b"]);
}

#[test]
fn fork_runs_a_block_alongside() {
    let (log, errors) = run_plain(
        "fork: [wait: 1. say: 'child']. say: 'parent'. wait: 2. say: 'late'.",
        400,
    );
    assert!(errors.is_empty());
    assert_eq!(
        log,
        ["[   0] say parent", "[ 120] say child", "[ 240] say late"]
    );
}

#[test]
fn strings_comments_and_negative_numbers() {
    assert_eq!(
        out("\"a comment\" say: 'it''s'. say: -3 + 5. say: 3 - -2."),
        ["say it's", "say 2", "say 5"]
    );
}

#[test]
fn the_error_for_a_type_mistake_names_the_value_and_the_message() {
    let e = run_error("say: 'closed' + 1.");
    assert_eq!(e, "level.st:1: 'closed' does not understand +");
}

#[test]
fn library_functions_every_after_and_wait_until() {
    let src = "n := 0. every: 1 do: [n := n + 1. say: n]. after: 2.5 do: [say: 'after']. waitUntil: [n >= 2]. say: 'done'.";
    let (log, errors) = run_plain(src, 400);
    assert!(errors.is_empty(), "{errors:?}");
    // waitUntil polls every 0.1 s (12 steps). At tick 240 the polling task runs
    // before the every:do: task bumps n to 2, so it sees it at the next poll.
    assert_eq!(
        log,
        [
            "[ 120] say 1",
            "[ 240] say 2",
            "[ 252] say done",
            "[ 300] say after",
            "[ 360] say 3"
        ]
    );
}
