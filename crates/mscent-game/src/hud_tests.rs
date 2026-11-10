//! Pixel snapshot tests. Each golden is an FNV-1a hash of the whole colour
//! buffer; the structural assertions next to it say what the pixels mean, so
//! a changed hash can be judged rather than blindly re-recorded.

use crate::test_util::{far_goal, game, open_level, start, v};
use crate::{Game, HudModel, Menu, MenuCommand, Phase, Screen, draw_hud, draw_menu};
use mscent_render::Framebuffer;

const W: u32 = 200;
const H: u32 = 120;
const BLACK: u32 = 0xff00_0000;

fn fb() -> Framebuffer {
    let mut f = Framebuffer::new(W, H).unwrap();
    f.clear(BLACK);
    f
}

fn fnv(f: &Framebuffer) -> u64 {
    f.pixels().iter().fold(0xcbf2_9ce4_8422_2325, |h, &p| {
        (h ^ u64::from(p)).wrapping_mul(0x100_0000_01b3)
    })
}

fn count(f: &Framebuffer, color: u32) -> usize {
    f.pixels().iter().filter(|&&p| p == color).count()
}

fn model(health: i32, hurt: bool) -> HudModel {
    HudModel {
        health,
        max_health: 100,
        enemies_left: 3,
        goal_distance: 42.7,
        speed: 12,
        hurt,
        phase: Phase::Playing,
        use_prompt: None,
        craft: Default::default(),
    }
}

#[test]
fn model_reflects_the_game() {
    let mut g: Game = game(&open_level(vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()]));
    let m = HudModel::from_game(&g);
    assert_eq!((m.health, m.enemies_left, m.hurt), (100, 0, false));
    assert!((m.goal_distance - (v(15.0, 15.0, 15.0)).length()).abs() < 1e-3);
    g.player.damage(30);
    assert_eq!(HudModel::from_game(&g).health, 70);
}

#[test]
fn bar_fill_tracks_health_and_clamps() {
    assert_eq!(model(100, false).bar_fill(), 100);
    assert_eq!(model(50, false).bar_fill(), 50);
    assert_eq!(model(0, false).bar_fill(), 0);
    assert_eq!(model(-20, false).bar_fill(), 0);
    assert_eq!(model(900, false).bar_fill(), 100);
}

#[test]
fn healthy_hud_pixels() {
    let mut f = fb();
    draw_hud(&mut f, &model(100, false));
    assert_eq!(
        f.pixel(W as i32 / 2 + 3, H as i32 / 2),
        Some(0xffff_ffff),
        "crosshair arm"
    );
    assert_eq!(f.pixel(0, 0), Some(BLACK), "no hurt border");
    assert_eq!(
        count(&f, 0xff30_d060),
        98 * 8,
        "full green bar inside its frame"
    );
    assert_eq!(fnv(&f), GOLDEN_HEALTHY);
}

#[test]
fn hurt_low_health_hud_pixels() {
    let mut f = fb();
    draw_hud(&mut f, &model(20, true));
    assert_ne!(f.pixel(0, 0), Some(BLACK), "hurt border tints the edge");
    assert_eq!(f.pixel(W as i32 / 2, 30), Some(BLACK), "centre stays clear");
    assert_eq!(count(&f, 0xff30_d060), 0);
    assert!(count(&f, 0xffe0_3030) > 0, "low health draws red");
    assert_eq!(fnv(&f), GOLDEN_HURT);
}

#[test]
fn menu_pixels_and_hidden_menu_draws_nothing() {
    let mut f = fb();
    let mut m = Menu::new();
    draw_menu(&mut f, &m);
    let first = fnv(&f);
    assert_ne!(first, fnv(&fb()));
    assert_eq!(first, GOLDEN_MENU);
    let mut g = fb();
    m.press(MenuCommand::Down);
    draw_menu(&mut g, &m);
    assert_ne!(fnv(&g), first, "selection moves the highlight");
    m.press(MenuCommand::Up);
    m.press(MenuCommand::Select);
    assert_eq!(m.screen(), Screen::Hidden);
    let mut blank = fb();
    draw_menu(&mut blank, &m);
    assert_eq!(
        fnv(&blank),
        fnv(&fb()),
        "a hidden menu leaves the frame alone"
    );
}

const GOLDEN_HEALTHY: u64 = 17709822797558256119;
const GOLDEN_HURT: u64 = 16931051451278821471;
const GOLDEN_MENU: u64 = 414795121215549997;
