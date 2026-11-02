//! Scripts asking for the wrong thing: load errors, bad requests, handlers that
//! fail, and hostile scripts running into the caps.

use crate::script_tests::{game_on, level, run, zone};
use crate::test_util::v;
use crate::{Arena, Event, Game, Input};
use mscent_script::Source;
use mscent_world::Entity;

#[test]
fn a_typo_in_a_name_or_message_stops_the_load_even_in_an_unfired_handler() {
    let mut game = Game::new(Arena::from_scripted_level(&level(Vec::new())).unwrap());
    for (src, wanted) in [
        ("[ mover: 'dor' ].", "did you mean 'door'"),
        ("[ (mover: 'door') moveToo: 'open' in: 1 ].", "moveTo:in:"),
        ("[ zone: 'door' ].", "the level has no such name"),
    ] {
        let text = src;
        let errors = game
            .load_script(&[Source {
                name: "bad.st",
                text,
            }])
            .unwrap_err();
        assert!(errors[0].to_string().starts_with("bad.st:1:"), "{errors:?}");
        assert!(errors[0].msg.contains(wanted), "{errors:?}");
    }
    assert!(!game.has_script(), "a failed load changes nothing");
}

/// Each runs at the top level, so its error has this script's line.
#[test]
fn a_bad_request_is_an_error_on_the_calling_line_not_a_game_state() {
    for (src, wanted) in [
        ("(mover: 'door') moveTo: 'nope' in: 1.", "no pose 'nope'"),
        (
            "(mover: 'door') moveTo: 'closed' in: -1.",
            "the time must be",
        ),
        (
            "(mover: 'door') moveTo: 'closed' in: 'soon'.",
            "must be a number",
        ),
        ("(mover: 'door') pose: 'p' move: 3.", "wanted a marker"),
        (
            "(mover: 'door') pose: 'p' move: (x: 1000000000 y: 0 z: 0).",
            "out of range",
        ),
        (
            "(mover: 'door') pose: 'closed' move: (x: 1 y: 0 z: 0).",
            "'closed'",
        ),
        (
            "(mover: 'door') pose: 'p' turn: (x: 0 y: 0 z: 0) degrees: 5 about: (x: 0 y: 0 z: 0).",
            "not zero",
        ),
        (
            "(mover: 'door') pose: 'p' turn: (x: 0 y: 0 z: 1) degrees: 270 about: (x: 0 y: 0 z: 0).",
            "at most 180",
        ),
        ("(mover: 'door') whenBlocked: 'crush'.", "needs an amount"),
        ("(mover: 'door') whenBlocked: 'bounce'.", "no such policy"),
        (
            "(mover: 'door') whenBlocked: 'crush' damage: -5.",
            "must not be negative",
        ),
        ("player moveTo: 'closed' in: 1.", "for a mover"),
        ("(mover: 'door') hp.", "player or an enemy"),
        ("(mover: 'door') onEnter: [ ].", "for a zone"),
        ("3 alive.", "tagged group or an enemy"),
        ("markers: 'nothing_like_this'.", "no markers named"),
        ("mover: 'do' , 'oor'.", "no mover named"),
        (
            "spawn: 'dragon' at: (x: 0 y: 0 z: 0) tag: 't'.",
            "no enemy kind",
        ),
        (
            "spawn: 'drone' at: (x: 100 y: 0 z: 0) tag: 't'.",
            "solid rock",
        ),
        ("spawn: 'drone' at: 5 tag: 't'.", "wanted a marker"),
        (
            "spawn: 'drone' at: (x: 0 y: 0 z: 0) tag: ''.",
            "1 to 32 bytes",
        ),
        ("defineEnemy: 'x' base: 7.", "must be 0"),
        ("defineEnemy: 'drone' base: 0.", "already defined"),
        ("(defineEnemy: 'x' base: 0) radius: 0.", "above zero"),
        ("(defineEnemy: 'x' base: 0) hp: -1.", "between 0"),
        ("player heal: -1.", "not be negative"),
        ("player give: ''.", "1 to 32 bytes"),
    ] {
        let mut game = game_on(&level(Vec::new()), src);
        let errors = game.take_script_errors();
        assert_eq!(errors.len(), 1, "{src}: {errors:?}");
        assert!(
            errors[0].to_string().starts_with("level.st:1:"),
            "{src}: {errors:?}"
        );
        assert!(errors[0].msg.contains(wanted), "{src}: {errors:?}");
    }
}

#[test]
fn a_handler_that_errors_stops_and_the_others_carry_on() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(zone(0.0), "hall", &[])];
    let src = "
        (zone: 'hall') onEnter: [ say: 'bad'. 1 / 0 ].
        (zone: 'hall') onEnter: [ say: 'good' ].";
    let mut game = game_on(&level(named), src);
    for _ in 0..3 {
        game.player.body.pos = v(0.0, 10.0, 0.0);
        run(&mut game, 2);
        game.player.body.pos = v(0.0, 0.0, 0.0);
        run(&mut game, 2);
    }
    let errors = game.take_script_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].line, 2);
    assert_eq!(game.take_messages(), ["bad", "good", "good", "good"]);
}

#[test]
fn hostile_scripts_hit_a_cap_rather_than_the_frame_budget() {
    for (src, wanted) in [
        (
            "[true] whileTrue: [ spawn: 'drone' at: (x: 0 y: 0 z: 0) tag: 't'. wait: 0 ].",
            "alive",
        ),
        (
            "[true] whileTrue: [ 50 timesRepeat: [ (zone: 'hall') onEnter: [ ] ]. wait: 0 ].",
            "too many handlers",
        ),
        (
            "n := 0. [true] whileTrue: [ n := n + 1. tagged: 't' , n. wait: 0 ].",
            "too many tags",
        ),
        (
            "n := 0. [true] whileTrue: [ n := n + 1. defineEnemy: 'k' , n base: 0. wait: 0 ].",
            "too many enemy kinds",
        ),
        (
            "n := 0. [true] whileTrue: [ n := n + 1. player give: 'i' , n. wait: 0 ].",
            "too many items",
        ),
    ] {
        let named: Vec<(Entity, &str, &[&str])> = vec![(zone(0.0), "hall", &[])];
        let mut game = game_on(&level(named), src);
        for _ in 0..1000 {
            // Immortal, so a level full of shooting enemies cannot end the test.
            game.player.health = crate::player::MAX_HEALTH;
            game.step(&Input::default());
        }
        let errors = game.take_script_errors();
        assert_eq!(errors.len(), 1, "{src}: {errors:?}");
        assert!(errors[0].msg.contains(wanted), "{src}: {errors:?}");
    }
}

#[test]
fn messages_and_errors_cannot_pile_up_without_limit() {
    let src = "[true] whileTrue: [ say: 'x'. wait: 0 ].";
    let mut game = game_on(&level(Vec::new()), src);
    run(&mut game, 500);
    assert_eq!(game.take_messages().len(), 64);
}

#[test]
fn a_level_with_no_script_tracks_no_zones_and_says_nothing() {
    let mut lv = level(vec![(zone(0.0), "hall", &[])]);
    lv.entities.push(crate::test_util::far_goal());
    let mut game = Game::new(Arena::from_level(&lv).unwrap());
    assert!(!game.has_script());
    game.player.body.pos = v(0.0, 10.0, 0.0);
    run(&mut game, 3);
    assert!(
        !game
            .take_events()
            .iter()
            .any(|e| matches!(e, Event::ZoneEntered(_)))
    );
    assert!(game.take_messages().is_empty());
}

#[test]
fn every_native_has_a_distinct_name() {
    let table = crate::natives();
    let mut names: Vec<&str> = table.iter().map(|n| n.0).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count);
}
