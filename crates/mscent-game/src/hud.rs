//! HUD model and drawing (M6.6). The only module that touches
//! `mscent_render`; everything it shows is copied into a plain
//! [`HudModel`] first, so the game stays testable without pixels.

use crate::game::{Game, Phase};
use crate::menu::Menu;
use crate::pilot::JET_FUEL;
use crate::player::MAX_HEALTH;
use mscent_render::{Framebuffer, draw_text, fill_rect, stroke_rect, text_size};

const WHITE: u32 = 0xffff_ffff;
const GREEN: u32 = 0xff30_d060;
const YELLOW: u32 = 0xffe0_c030;
const RED: u32 = 0xffe0_3030;
const FRAME: u32 = 0xffc0_c0c0;
const BAR_BG: u32 = 0xff20_2020;
const HURT_TINT: u32 = 0x60ff_0000;
const DIM: u32 = 0xa000_0000;

const MARGIN: i32 = 8;
const BAR_W: u32 = 100;
const BAR_H: u32 = 10;

/// Everything the HUD shows, as plain numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudModel {
    pub health: i32,
    pub max_health: i32,
    pub enemies_left: usize,
    pub goal_distance: f32,
    /// Speed in whole units per second.
    pub speed: u32,
    pub hurt: bool,
    pub phase: Phase,
    /// Something usable is in reach; how far along holding the key is.
    pub use_prompt: Option<f32>,
    pub craft: CraftHud,
}

/// The ship-and-pilot part of the HUD.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CraftHud {
    /// Jetpack fuel, 0 to 1, while on foot.
    pub fuel: Option<f32>,
    /// What the board key does now, if anything.
    pub board: Option<&'static str>,
}

impl CraftHud {
    pub fn from_game(g: &Game) -> Self {
        let board = if g.on_foot() {
            g.can_board().then_some("B  BOARD")
        } else {
            g.ship_landed().then_some("B  LEAVE SHIP")
        };
        Self {
            fuel: g.pilot.map(|p| p.fuel / JET_FUEL),
            board,
        }
    }
}

impl HudModel {
    pub fn from_game(g: &Game) -> Self {
        Self {
            health: g.player.health,
            max_health: MAX_HEALTH,
            enemies_left: g.enemies_alive(),
            goal_distance: g.goal_distance(),
            speed: g.pilot.map_or(g.player.body.vel, |p| p.body.vel).length() as u32,
            hurt: g.hurt_steps > 0,
            phase: g.phase,
            use_prompt: g.use_prompt(),
            craft: CraftHud::from_game(g),
        }
    }

    /// Filled bar width in pixels for the current health.
    pub fn bar_fill(&self) -> u32 {
        let max = i64::from(self.max_health.max(1));
        let hp = i64::from(self.health).clamp(0, max);
        (hp * i64::from(BAR_W) / max) as u32
    }

    fn bar_color(&self) -> u32 {
        // i64: the model is plain public data, so `health * 4` may overflow i32.
        match i64::from(self.health) * 4 / i64::from(self.max_health.max(1)) {
            h if h >= 2 => GREEN,
            1 => YELLOW,
            _ => RED,
        }
    }
}

/// Draws the in-game overlay on top of an already rendered frame.
pub fn draw_hud(fb: &mut Framebuffer, m: &HudModel) {
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    if m.hurt {
        stroke_rect(fb, 0, 0, w as u32, h as u32, 6, HURT_TINT);
    }
    draw_crosshair(fb);
    let y = h - MARGIN - BAR_H as i32;
    fill_rect(fb, MARGIN, y, BAR_W, BAR_H, BAR_BG);
    fill_rect(fb, MARGIN, y, m.bar_fill(), BAR_H, m.bar_color());
    stroke_rect(fb, MARGIN, y, BAR_W, BAR_H, 1, FRAME);
    draw_text(
        fb,
        MARGIN,
        y - 10,
        &format!("HP {}", m.health.max(0)),
        WHITE,
        1,
    );
    let info = format!(
        "ENEMIES {}  GOAL {}  SPD {}",
        m.enemies_left,
        goal_text(m.goal_distance),
        m.speed
    );
    draw_text(fb, MARGIN, MARGIN, &info, WHITE, 1);
    if let Some(progress) = m.use_prompt {
        draw_use_prompt(fb, progress);
    }
    draw_craft(fb, &m.craft);
}

/// The jetpack's fuel beside the health bar, and the board key's prompt.
fn draw_craft(fb: &mut Framebuffer, c: &CraftHud) {
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    if let Some(fuel) = c.fuel {
        let (x, y) = (MARGIN * 2 + BAR_W as i32, h - MARGIN - BAR_H as i32);
        fill_rect(fb, x, y, BAR_W, BAR_H, BAR_BG);
        let fill = (BAR_W as f32 * fuel.clamp(0.0, 1.0)) as u32;
        fill_rect(fb, x, y, fill, BAR_H, 0xff40_a0f0);
        stroke_rect(fb, x, y, BAR_W, BAR_H, 1, FRAME);
        draw_text(fb, x, y - 10, "JET", WHITE, 1);
    }
    if let Some(prompt) = c.board {
        let tw = text_size(prompt, 1).0 as i32;
        draw_text(fb, (w - tw) / 2, h / 2 + 44, prompt, YELLOW, 1);
    }
}

fn draw_crosshair(fb: &mut Framebuffer) {
    let (cx, cy) = (fb.width() as i32 / 2, fb.height() as i32 / 2);
    fill_rect(fb, cx - 5, cy, 4, 1, WHITE);
    fill_rect(fb, cx + 2, cy, 4, 1, WHITE);
    fill_rect(fb, cx, cy - 5, 1, 4, WHITE);
    fill_rect(fb, cx, cy + 2, 1, 4, WHITE);
}

/// Draws the menu over a dimmed frame. Draws nothing when hidden.
pub fn draw_menu(fb: &mut Framebuffer, menu: &Menu) {
    if menu.items().is_empty() {
        return;
    }
    let (w, h) = (fb.width(), fb.height());
    fill_rect(fb, 0, 0, w, h, DIM);
    let scale = 2;
    centered(fb, menu.title(), h as i32 / 4, WHITE, scale + 1);
    let line = text_size("X", scale).1 as i32 + 6;
    for (i, label) in menu.items().iter().enumerate() {
        let selected = i == menu.selected();
        let text = if selected {
            format!("> {label} <")
        } else {
            (*label).to_string()
        };
        let color = if selected { YELLOW } else { FRAME };
        centered(fb, &text, h as i32 / 2 + i as i32 * line, color, scale);
    }
}

fn centered(fb: &mut Framebuffer, text: &str, y: i32, color: u32, scale: u32) {
    let tw = text_size(text, scale).0 as i32;
    draw_text(fb, (fb.width() as i32 - tw) / 2, y, text, color, scale);
}

/// A scripted level can have no goal, and so no distance to one.
fn goal_text(distance: f32) -> String {
    if distance == f32::MAX {
        "-".to_string()
    } else {
        (distance as u32).to_string()
    }
}

/// "HOLD G" under the crosshair, with a bar while it is held.
fn draw_use_prompt(fb: &mut Framebuffer, progress: f32) {
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    let label = "HOLD G";
    let (cx, y) = (w / 2, h / 2 + 24);
    draw_text(fb, cx - 18, y, label, WHITE, 1);
    if progress > 0.0 {
        fill_rect(fb, cx - 30, y + 12, 60, 4, 0xff30_3030);
        fill_rect(
            fb,
            cx - 30,
            y + 12,
            (60.0 * progress) as u32,
            4,
            0xff40_e0f0,
        );
    }
}
