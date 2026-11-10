//! The scale figures in a 2D view: the ship's collision sphere at the level's
//! size and the pilot standing beside it, drawn at the player start and at a
//! Scale entity, so a door or a ceiling can be judged against both as it is
//! drawn. The sizes come from the game's scale guide, which the simulation's
//! own numbers make, so the figures cannot drift from play.

use mscent_game::ScaleGuide;
use mscent_world::Entity;
use xui::Color;
use xui::geometry::Rect;

use super::{Pen, Scene, clamp};

const SHIP: Color = Color::rgb(120, 200, 240);
const PILOT: Color = Color::rgb(250, 210, 90);

/// The level's ship size: its first Scale entity's, or the stock 1.
pub(super) fn ship_scale(scene: &Scene) -> f32 {
    let mut scales = scene.state.doc.entities().iter();
    scales
        .find_map(|e| match e.entity {
            Entity::Scale { ship, .. } => Some(ship),
            _ => None,
        })
        .unwrap_or(1.0)
}

/// The ship's sphere centred on `at`, resting on the ground, and the pilot
/// standing on that ground beside it.
pub(super) fn figures(pen: &mut Pen, scene: &Scene, at: [f64; 3], ship_scale: f32) {
    let guide = ScaleGuide::new(ship_scale);
    let (a, b) = scene.view.axes();
    let r = f64::from(guide.ship_radius);
    let c = pen.pt([at[a], at[b]]);
    let rr = (r * pen.cam.scale) as f32;
    if rr < 4000.0 {
        pen.canvas.stroke_ellipse(c, rr, rr, SHIP, pen.s);
    }
    // The pilot a metre clear of the hull, along the view's first axis.
    let half = f64::from(guide.pilot_width) * 0.5;
    let x = at[a] + r + 1.0 + half;
    let (lo, hi) = if b == 2 {
        let ground = at[2] - r;
        (
            [x - half, ground],
            [x + half, ground + f64::from(guide.pilot_height)],
        )
    } else {
        ([x - half, at[b] - half], [x + half, at[b] + half])
    };
    let (p, q) = (pen.pt(lo), pen.pt(hi));
    let rect = Rect::new(p.x.min(q.x), p.y.min(q.y), p.x.max(q.x), p.y.max(q.y));
    // A figure far wider than any screen is a view zoomed far in: skip it.
    if rect.right - rect.left < clamp(1.0e5) {
        pen.canvas.stroke_rect(rect, PILOT, pen.s);
    }
}
