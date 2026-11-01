//! Drawing of one orthographic view: grid, brush outlines, entities and
//! selection handles.

use mscent_world::{Entity, Op};
use xui::Color;
use xui::backend::{Canvas, Rgba, TextStyle};
use xui::geometry::{Point, Rect};
use xui::theme::Theme;
use xui::units::Dip;

use crate::model::entity::pos;
use crate::model::pointer::handles;
use crate::model::{Camera2D, EditorState, Selection, Shape, View};

/// Colour of an additive brush.
const ADD: Color = Color::rgb(235, 150, 55);
/// Colour of a subtractive brush.
const SUBTRACT: Color = Color::rgb(55, 150, 230);

/// Everything one paint needs.
pub struct Scene<'a> {
    pub state: &'a EditorState,
    pub view: View,
    pub cam: Camera2D,
    pub theme: Theme,
}

fn clamp(v: f64) -> i32 {
    v.clamp(-1.0e6, 1.0e6) as i32
}

fn rgba(c: Color, a: u8) -> Rgba {
    Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a,
    }
}

struct Pen<'a> {
    canvas: &'a mut dyn Canvas,
    cam: Camera2D,
    size: (f64, f64),
    /// Top-left of the canvas in its own coordinates.
    origin: (f64, f64),
    /// Device pixels per design pixel.
    s: f32,
}

impl Pen<'_> {
    fn pt(&self, p: [f64; 2]) -> Point {
        let (x, y) = self.cam.to_screen(p, self.size);
        Point::new(clamp(x + self.origin.0), clamp(y + self.origin.1))
    }

    fn rect(&self, lo: [f64; 2], hi: [f64; 2]) -> Rect {
        let a = self.pt([lo[0], hi[1]]);
        let b = self.pt([hi[0], lo[1]]);
        Rect::new(a.x, a.y, b.x.max(a.x + 1), b.y.max(a.y + 1))
    }
}

pub fn paint(canvas: &mut dyn Canvas, scene: &Scene) {
    let bounds = canvas.bounds();
    let theme = scene.theme;
    let bg = theme.input_background;
    canvas.clear(bg);
    canvas.push_clip(bounds);
    let size = (bounds.width() as f64, bounds.height() as f64);
    let s = canvas.dpi() as f32 / 96.0;
    let mut pen = Pen {
        canvas,
        cam: scene.cam,
        size,
        origin: (bounds.left as f64, bounds.top as f64),
        s,
    };
    grid(&mut pen, scene);
    brushes(&mut pen, scene);
    entities(&mut pen, scene);
    creating(&mut pen, scene);
    slicing(&mut pen, scene);
    corners(&mut pen, scene);
    label(&mut pen, scene, bounds);
    pen.canvas.pop_clip();
    pen.canvas.stroke_rect(bounds, theme.border, 1.0);
}

fn grid(pen: &mut Pen, scene: &Scene) {
    let theme = scene.theme;
    let (a, b) = (scene.cam.center, pen.size);
    let lo = pen.cam.to_world(0.0, b.1, b);
    let hi = pen.cam.to_world(b.0, 0.0, b);
    let _ = a;
    let mut step = scene.state.grid;
    while step * pen.cam.scale < 9.0 * pen.s as f64 {
        step *= 2.0;
    }
    let bg = theme.input_background;
    let minor = bg.lerp(theme.text, 0.08);
    let major = bg.lerp(theme.text, 0.17);
    let axis = bg.lerp(theme.text, 0.45);
    let line = |pen: &mut Pen, vertical: bool, w: f64, color: Color| {
        let (p, q) = if vertical {
            (pen.pt([w, lo[1]]), pen.pt([w, hi[1]]))
        } else {
            (pen.pt([lo[0], w]), pen.pt([hi[0], w]))
        };
        pen.canvas.draw_line(p, q, color, 1.0);
    };
    for vertical in [true, false] {
        let (from, to) = if vertical {
            (lo[0], hi[0])
        } else {
            (lo[1], hi[1])
        };
        let first = (from / step).floor() as i64;
        let last = (to / step).ceil() as i64;
        for i in first..=last.min(first + 4000) {
            let w = i as f64 * step;
            let color = if i == 0 {
                axis
            } else if i % 8 == 0 {
                major
            } else {
                minor
            };
            line(pen, vertical, w, color);
        }
    }
}

fn brushes(pen: &mut Pen, scene: &Scene) {
    let (a, b) = scene.view.axes();
    let selected = match scene.state.selection {
        Some(Selection::Brush(id)) => Some(id),
        _ => None,
    };
    for br in scene.state.doc.brushes() {
        let (lo, hi) = br.bounds();
        let r = pen.rect([lo[a], lo[b]], [hi[a], hi[b]]);
        let color = match br.op {
            Op::Add => ADD,
            Op::Subtract => SUBTRACT,
        };
        let alpha = if br.op == Op::Add { 70 } else { 28 };
        pen.canvas.fill_rect_rgba(r, rgba(color, alpha));
        pen.canvas.stroke_rect(r, color, pen.s);
    }
    let Some(br) = selected.and_then(|id| scene.state.doc.brush(id)) else {
        return;
    };
    let (lo, hi) = br.bounds();
    let r = pen.rect([lo[a], lo[b]], [hi[a], hi[b]]);
    let accent = scene.theme.accent;
    pen.canvas.fill_rect_rgba(r, rgba(accent, 90));
    pen.canvas
        .stroke_rect(r.shrink(-1), Color::rgb(255, 255, 255), 1.0);
    pen.canvas.stroke_rect(r, accent, 2.0 * pen.s);
    if matches!(br.shape, Shape::Box { .. }) {
        let h = (3.5 * pen.s) as i32;
        for (_, p) in handles([lo[a], lo[b]], [hi[a], hi[b]]) {
            let c = pen.pt(p);
            let hr = Rect::new(c.x - h, c.y - h, c.x + h + 1, c.y + h + 1);
            pen.canvas.fill_rect(hr, Color::rgb(255, 255, 255));
            pen.canvas.stroke_rect(hr, accent, 1.0);
        }
    }
}

fn entities(pen: &mut Pen, scene: &Scene) {
    let (a, b) = scene.view.axes();
    let selected = match scene.state.selection {
        Some(Selection::Entity(id)) => Some(id),
        _ => None,
    };
    let r = 4.5 * pen.s;
    for e in scene.state.doc.entities() {
        let q = pos(&e.entity);
        let c = pen.pt([q[a], q[b]]);
        match e.entity {
            Entity::Light { color, radius, .. } => {
                let col = Color::rgb(
                    (color.x * 255.0) as u8,
                    (color.y * 255.0) as u8,
                    (color.z * 255.0) as u8,
                );
                let rr = (radius as f64 * pen.cam.scale) as f32;
                if rr < 4000.0 && selected == Some(e.id) {
                    pen.canvas.stroke_ellipse(c, rr, rr, col, 1.0);
                }
                pen.canvas.fill_ellipse(c, r, r, col);
                pen.canvas
                    .stroke_ellipse(c, r, r, Color::rgb(60, 50, 20), 1.0);
            }
            Entity::PlayerStart { yaw, .. } => {
                let g = Color::rgb(60, 190, 90);
                pen.canvas.fill_ellipse(c, r, r, g);
                if scene.view == View::Top {
                    let tip = Point::new(
                        c.x + (yaw.cos() * r * 2.2) as i32,
                        c.y - (yaw.sin() * r * 2.2) as i32,
                    );
                    pen.canvas.draw_line(c, tip, g, 2.0 * pen.s);
                }
            }
            Entity::Enemy { .. } => {
                let i = r as i32;
                let rr = Rect::new(c.x - i, c.y - i, c.x + i + 1, c.y + i + 1);
                pen.canvas.fill_rect(rr, Color::rgb(215, 55, 50));
                pen.canvas.draw_line(
                    Point::new(rr.left, rr.top),
                    Point::new(rr.right, rr.bottom),
                    Color::rgb(255, 230, 230),
                    1.0,
                );
                pen.canvas.draw_line(
                    Point::new(rr.left, rr.bottom),
                    Point::new(rr.right, rr.top),
                    Color::rgb(255, 230, 230),
                    1.0,
                );
            }
            Entity::Goal { radius, .. } => {
                let i = (r * 1.3) as i32;
                let diamond = [
                    Point::new(c.x, c.y - i),
                    Point::new(c.x + i, c.y),
                    Point::new(c.x, c.y + i),
                    Point::new(c.x - i, c.y),
                ];
                pen.canvas.fill_polygon(&diamond, Color::rgb(190, 90, 220));
                let rr = (radius as f64 * pen.cam.scale) as f32;
                if rr < 4000.0 && selected == Some(e.id) {
                    pen.canvas
                        .stroke_ellipse(c, rr, rr, Color::rgb(190, 90, 220), 1.0);
                }
            }
        }
        if selected == Some(e.id) {
            let ring = r + 3.0 * pen.s;
            pen.canvas
                .stroke_ellipse(c, ring, ring, scene.theme.accent, 2.0 * pen.s);
        }
    }
}

fn creating(pen: &mut Pen, scene: &Scene) {
    let Some((view, lo, hi)) = scene.state.create_rect() else {
        return;
    };
    if view != scene.view {
        return;
    }
    let r = pen.rect(lo, hi);
    let color = match scene.state.new_op {
        Op::Add => ADD,
        Op::Subtract => SUBTRACT,
    };
    pen.canvas.fill_rect_rgba(r, rgba(color, 60));
    pen.canvas.stroke_rect(r, color, 2.0 * pen.s);
}

/// The slice line, with ticks on the side that stays.
fn slicing(pen: &mut Pen, scene: &Scene) {
    let Some(cut) = scene.state.slice_line() else {
        return;
    };
    if cut.view != scene.view {
        return;
    }
    let (p, q) = (pen.pt(cut.a), pen.pt(cut.b));
    let color = Color::rgb(235, 60, 60);
    pen.canvas.draw_line(p, q, color, 2.0 * pen.s);
    let (dx, dy) = ((q.x - p.x) as f32, (q.y - p.y) as f32);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return;
    }
    // Screen y points down, so the right of the drawn direction is (-dy, dx).
    let (nx, ny) = (-dy / len, dx / len);
    let keep = scene.state.keep;
    for (side, show) in [
        (1.0, keep != crate::model::Keep::Left),
        (-1.0, keep != crate::model::Keep::Right),
    ] {
        if !show {
            continue;
        }
        let mid = Point::new((p.x + q.x) / 2, (p.y + q.y) / 2);
        let tip = Point::new(
            mid.x + (nx * side * 10.0 * pen.s) as i32,
            mid.y + (ny * side * 10.0 * pen.s) as i32,
        );
        pen.canvas.draw_line(mid, tip, color, pen.s);
    }
}

/// The selected brush's corners while the vertex tool is active.
fn corners(pen: &mut Pen, scene: &Scene) {
    let h = (3.5 * pen.s) as i32;
    for v in scene.state.corner_handles(scene.view) {
        let c = pen.pt(v);
        let r = Rect::new(c.x - h, c.y - h, c.x + h + 1, c.y + h + 1);
        pen.canvas.fill_rect(r, Color::rgb(255, 235, 80));
        pen.canvas.stroke_rect(r, Color::rgb(60, 50, 20), 1.0);
    }
}

fn label(pen: &mut Pen, scene: &Scene, bounds: Rect) {
    let mut style = TextStyle::new(scene.theme.text_secondary, Dip(11.0));
    style.weight = xui::backend::TextWeight::BOLD;
    let r = Rect::new(
        bounds.left + 8,
        bounds.top + 4,
        bounds.right - 8,
        bounds.top + 24,
    );
    pen.canvas.draw_text(scene.view.name(), r, &style);
}
