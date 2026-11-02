//! Props, pickups, the use key and factions, through the real `Game::step`.

use crate::script_tests::{game_on, level, no_errors, run};
use crate::test_util::{enemy, v};
use crate::{Event, Faction, Game, Input};
use mscent_world::Entity;

fn marker(x: f32, y: f32) -> Entity {
    Entity::Marker {
        pos: v(x, y, 0.0),
        yaw: 0.0,
    }
}

fn firing() -> Input {
    Input {
        fire: true,
        ..Input::default()
    }
}

fn using() -> Input {
    Input {
        use_key: true,
        ..Input::default()
    }
}

fn hold(game: &mut Game, input: &Input, steps: usize) {
    for _ in 0..steps {
        game.step(input);
    }
}

#[test]
fn a_prop_with_health_is_shot_and_reports_it() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(4.0, 0.0), "spot", &[])];
    let src = "
        t := spawn: 'switch' at: (marker: 'spot') tag: 'targets'.
        t onHit: [ say: 'hit' ].
        t onDestroyed: [:it | say: 'gone ' , it alive ].
        (tagged: 'targets') onAllDestroyed: [ say: 'all gone' ].";
    let mut game = game_on(&level(named), src);
    assert_eq!(game.props.len(), 1);
    hold(&mut game, &firing(), 3);
    assert_eq!(game.take_messages(), ["hit", "gone 0", "all gone"]);
    assert!(!game.props[0].present);
    no_errors(&mut game);
}

#[test]
fn a_prop_with_no_health_lets_shots_through_unless_it_reports_hits() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(4.0, 0.0), "spot", &[])];
    let plain = "spawn: 'keycard' at: (marker: 'spot') tag: 'x'.
        (mover: 'door') onHit: [ say: 'door hit' ].";
    let mut game = game_on(&level(named.clone()), plain);
    hold(&mut game, &firing(), 3);
    assert_eq!(game.take_messages(), ["door hit"], "it passed through");

    let reporting = "(spawn: 'keycard' at: (marker: 'spot') tag: 'x') onHit: [ say: 'stopped' ].
        (mover: 'door') onHit: [ say: 'door hit' ].";
    let mut game = game_on(&level(named), reporting);
    hold(&mut game, &firing(), 3);
    assert_eq!(game.take_messages(), ["stopped"]);
    assert!(game.props[0].present, "no health to lose");
}

const KEYCARD: &str = "
    card := spawn: 'keycard' at: (marker: 'spot') tag: 'key'.
    card onPickup: [ player give: 'red'. say: 'Red keycard' ].";

#[test]
fn a_keycard_is_picked_up_by_touching_it_once() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(5.0, 0.0), "spot", &[])];
    let mut game = game_on(&level(named), KEYCARD);
    run(&mut game, 5);
    assert!(!game.has_item("red"), "out of reach");
    game.player.body.pos = v(5.0, 0.5, 0.0);
    run(&mut game, 3);
    assert!(game.has_item("red"));
    assert!(!game.props[0].present);
    game.player.body.pos = v(0.0, 0.0, 0.0);
    run(&mut game, 2);
    game.player.body.pos = v(5.0, 0.5, 0.0);
    run(&mut game, 2);
    assert_eq!(game.take_messages(), ["Red keycard"], "only once");
    no_errors(&mut game);
}

#[test]
fn a_disabled_prop_cannot_be_picked_up_until_enabled() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(5.0, 0.0), "spot", &[])];
    let src = format!("{KEYCARD} card disable. after: 0.1 do: [ card enable ].");
    let mut game = game_on(&level(named), &src);
    game.player.body.pos = v(5.0, 0.5, 0.0);
    run(&mut game, 2);
    assert!(!game.has_item("red"));
    run(&mut game, 20);
    assert!(game.has_item("red"));
}

const TERMINAL: &str = "
    terminal := spawn: 'terminal' at: (marker: 'spot') tag: 'term'.
    terminal onUse: [
        (tagged: 'drones') setFaction: 'friendly'.
        say: 'Override'.
    ] hold: 1.";

fn terminal_level() -> Vec<(Entity, &'static str, &'static [&'static str])> {
    vec![
        (marker(4.0, 0.0), "spot", &[]),
        (enemy(v(0.0, 8.0, 0.0)), "", &["drones"]),
        (enemy(v(0.0, 8.0, 3.0)), "", &["drones"]),
    ]
}

#[test]
fn holding_the_use_key_on_a_terminal_turns_enemies_friendly() {
    let mut game = game_on(&level(terminal_level()), TERMINAL);
    // Facing it from the start, within reach.
    game.player.body.pos = v(1.5, 0.0, 0.0);
    game.player.orientation =
        mscent_math::Quat::from_axis_angle(v(0.0, 1.0, 0.0), -core::f32::consts::FRAC_PI_2);
    run(&mut game, 1);
    assert_eq!(game.use_prompt(), Some(0.0), "the prompt shows");
    hold(&mut game, &using(), 60);
    let half = game.use_prompt().unwrap();
    assert!((0.4..0.6).contains(&half), "{half}");
    assert_eq!(game.enemies[0].faction, Faction::Hostile, "not yet");
    hold(&mut game, &using(), 70);
    assert_eq!(game.enemies[0].faction, Faction::Friendly);
    assert_eq!(game.enemies[1].faction, Faction::Friendly);
    assert_eq!(game.take_messages(), ["Override"]);
    assert_eq!(
        game.enemies_alive(),
        0,
        "the HUD counts only those against you"
    );
    // Holding on does not fire it again; a new press does.
    hold(&mut game, &using(), 240);
    assert!(game.take_messages().is_empty());
    run(&mut game, 1);
    hold(&mut game, &using(), 130);
    assert_eq!(game.take_messages(), ["Override"]);
    no_errors(&mut game);
}

#[test]
fn letting_go_early_or_walking_off_loses_the_progress() {
    let mut game = game_on(&level(terminal_level()), TERMINAL);
    game.player.body.pos = v(1.5, 0.0, 0.0);
    game.player.orientation =
        mscent_math::Quat::from_axis_angle(v(0.0, 1.0, 0.0), -core::f32::consts::FRAC_PI_2);
    run(&mut game, 1);
    hold(&mut game, &using(), 100);
    run(&mut game, 1);
    assert_eq!(game.use_prompt(), Some(0.0));
    hold(&mut game, &using(), 100);
    game.player.body.pos = v(-15.0, 0.0, 0.0);
    hold(&mut game, &using(), 100);
    assert_eq!(game.use_prompt(), None, "out of reach");
    assert!(game.take_messages().is_empty());
}

#[test]
fn a_terminal_behind_the_player_or_a_wall_cannot_be_used() {
    let mut game = game_on(&level(terminal_level()), TERMINAL);
    game.player.body.pos = v(1.5, 0.0, 0.0);
    // Looking away, along -X.
    game.player.orientation =
        mscent_math::Quat::from_axis_angle(v(0.0, 1.0, 0.0), core::f32::consts::FRAC_PI_2);
    hold(&mut game, &using(), 200);
    assert_eq!(game.use_prompt(), None);
    assert!(game.take_messages().is_empty());
}

#[test]
fn friendly_enemies_fight_hostile_ones_and_leave_the_player_alone() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (enemy(v(-6.0, 0.0, 0.0)), "", &["ours"]),
        (enemy(v(-6.0, 5.0, 0.0)), "", &["theirs"]),
    ];
    let src = "(tagged: 'ours') setFaction: 'friendly'.
        (tagged: 'theirs') onAllDestroyed: [ say: 'they fell' ].";
    let mut game = game_on(&level(named), src);
    // The player is far off behind the door, so they only have each other.
    game.player.body.pos = v(17.0, 0.0, 0.0);
    let mut hit = false;
    for _ in 0..2400 {
        game.step(&Input::default());
        hit |= game
            .take_events()
            .iter()
            .any(|e| matches!(e, Event::EnemyHit(_)));
    }
    assert!(hit, "they fought");
    assert!(!game.enemies[1].alive() || !game.enemies[0].alive());
    assert_eq!(game.player.health, crate::player::MAX_HEALTH);
    no_errors(&mut game);
}

#[test]
fn a_friendly_can_turn_hostile_again() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(enemy(v(5.0, 1.0, 0.0)), "pal", &[])];
    let src = "(enemy: 'pal') setFaction: 'friendly'.
        after: 1 do: [ (enemy: 'pal') setFaction: 'hostile' ].";
    let mut game = game_on(&level(named), src);
    run(&mut game, 100);
    assert_eq!(game.player.health, crate::player::MAX_HEALTH, "a friend");
    run(&mut game, 480);
    assert!(
        game.player.health < crate::player::MAX_HEALTH,
        "not any more"
    );
}

#[test]
fn a_custom_prop_kind_takes_its_settings() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(4.0, 0.0), "spot", &[])];
    let src = "
        kind := defineProp: 'barrel'.
        kind mesh: 'crate'.
        kind radius: 2.
        kind hp: 50.
        spawn: 'barrel' at: (marker: 'spot') tag: 'b'.";
    let mut game = game_on(&level(named), src);
    assert_eq!(game.props[0].radius, 2.0);
    assert_eq!(game.props[0].hp, 50);
    no_errors(&mut game);
}

#[test]
fn destroy_fires_the_same_handlers_as_a_shot() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(4.0, 0.0), "spot", &[])];
    let src = "
        c := spawn: 'crate' at: (marker: 'spot') tag: 'c'.
        (tagged: 'c') onAllDestroyed: [ say: 'cleared' ].
        after: 0.1 do: [ c destroy. c destroy ].";
    let mut game = game_on(&level(named), src);
    run(&mut game, 30);
    assert_eq!(game.take_messages(), ["cleared"]);
    no_errors(&mut game);
}

#[test]
fn a_group_going_all_at_once_says_so_once() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(4.0, 0.0), "spot", &[])];
    let src = "
        3 timesRepeat: [ spawn: 'crate' at: (marker: 'spot') tag: 'c' ].
        (tagged: 'c') onAllDestroyed: [ say: 'cleared' ].
        after: 0.1 do: [ (tagged: 'c') members do: [:m | m destroy] ].";
    let mut game = game_on(&level(named), src);
    run(&mut game, 30);
    assert_eq!(game.take_messages(), ["cleared"]);
    no_errors(&mut game);
}

#[test]
fn a_friendly_behind_a_door_does_not_shield_the_player_in_the_open() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (enemy(v(13.0, 0.0, 0.0)), "foe", &[]),
        (enemy(v(16.0, 0.0, 0.0)), "", &["pal"]),
    ];
    let src = "(tagged: 'pal') setFaction: 'friendly'.
        (tagged: 'pal') disable.";
    let mut game = game_on(&level(named), src);
    game.player.body.pos = v(6.0, 0.0, 0.0);
    run(&mut game, 300);
    assert!(game.player.health < crate::player::MAX_HEALTH);
    assert!(game.enemies[1].alive());
    no_errors(&mut game);
}

/// What the shots of this step struck, by who fired them.
fn struck_by(game: &mut Game) -> Vec<(crate::Shooter, crate::Struck)> {
    let events = game.take_events().into_iter();
    events
        .filter_map(|e| match e {
            crate::Event::Shot {
                shooter, struck, ..
            } => Some((shooter, struck)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_shot_at_a_prop_says_it_struck_the_prop() {
    let named: Vec<(Entity, &str, &[&str])> = vec![(marker(4.0, 0.0), "spot", &[])];
    let src = "spawn: 'crate' at: (marker: 'spot') tag: 'c'.";
    let mut game = game_on(&level(named), src);
    game.take_events();
    game.step(&crate::Input {
        fire: true,
        ..crate::Input::default()
    });
    let struck = struck_by(&mut game);
    assert!(
        struck.contains(&(crate::Shooter::Player, crate::Struck::Prop(0))),
        "{struck:?}"
    );
    no_errors(&mut game);
}

#[test]
fn a_friendly_shooting_a_foe_shows_it_as_a_shot_at_that_foe() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (enemy(v(8.0, 0.0, 0.0)), "foe", &[]),
        (enemy(v(12.0, 0.0, 0.0)), "", &["pal"]),
    ];
    let src = "(tagged: 'pal') setFaction: 'friendly'.";
    let mut game = game_on(&level(named), src);
    let mut seen = Vec::new();
    for _ in 0..600 {
        game.step(&crate::Input::default());
        seen.extend(struck_by(&mut game));
    }
    assert!(
        seen.contains(&(crate::Shooter::Enemy(1), crate::Struck::Enemy(0))),
        "{seen:?}"
    );
    no_errors(&mut game);
}

#[test]
fn bad_prop_requests_are_errors_on_their_line() {
    for (src, wanted) in [
        ("3 onPickup: [ ].", "for a prop"),
        ("3 onUse: [ ] hold: 1.", "for a prop"),
        ("(mover: 'door') onDestroyed: [ ].", "prop or an enemy"),
        ("(mover: 'door') destroy.", "prop or an enemy"),
        ("player setFaction: 'friendly'.", "tagged group or an enemy"),
        ("(tagged: 't') setFaction: 'purple'.", "no such faction"),
        (
            "(spawn: 'terminal' at: (x: 0 y: 0 z: 0) tag: 't') onUse: [ ] hold: -1.",
            "between 0",
        ),
        ("(defineProp: 'p') mesh: 'dragon'.", "no mesh"),
        ("(defineProp: 'p') radius: 0.", "above zero"),
        ("(defineProp: 'p') hp: -1.", "between 0"),
        ("defineProp: 'switch'.", "already defined"),
        ("defineProp: ''.", "1 to 32"),
    ] {
        let mut game = game_on(&level(Vec::new()), src);
        let errors = game.take_script_errors();
        assert_eq!(errors.len(), 1, "{src}: {errors:?}");
        assert!(errors[0].msg.contains(wanted), "{src}: {errors:?}");
    }
}

#[test]
fn a_prop_flood_hits_a_cap() {
    let src = "[true] whileTrue: [ 50 timesRepeat: [ spawn: 'crate' at: (x: 0 y: 0 z: 0) tag: 't' ]. wait: 0 ].";
    let mut game = game_on(&level(Vec::new()), src);
    run(&mut game, 100);
    let errors = game.take_script_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].msg.contains("too many props"), "{errors:?}");
}

#[test]
fn a_restart_clears_props_and_factions() {
    let named: Vec<(Entity, &str, &[&str])> = vec![
        (marker(4.0, 0.0), "spot", &[]),
        (enemy(v(0.0, 8.0, 0.0)), "e", &[]),
    ];
    let src = "spawn: 'crate' at: (marker: 'spot') tag: 't'.
        (enemy: 'e') setFaction: 'friendly'.
        spawn: 'crate' at: (marker: 'spot') tag: 't'.";
    let mut game = game_on(&level(named), src);
    assert_eq!(game.props.len(), 2);
    game.restart();
    assert_eq!(
        game.props.len(),
        2,
        "the script placed them afresh, not on top"
    );
    assert_eq!(game.enemies[0].faction, Faction::Friendly);
}
