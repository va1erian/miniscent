//! The brush section of the parameter panel, shown with a block that has strokes.
//! The brush is the workspace's, not the recipe's: these widgets change what the
//! next stroke is drawn with and nothing that is drawn, and take no undo step.

use mscent_texgen::ops::STROKE_RADIUS;
use xui::prelude::*;

use super::params::{SWATCHES, number, tidy};
use super::{BrushEdit, TexMsg};
use crate::texstudio::Brush;
use crate::ui::Msg;

/// A slider and the number beside it, which show one value.
#[derive(Default)]
struct Pair {
    slider: Handle<Slider<Msg>>,
    field: Handle<NumberField<Msg>>,
}

impl Pair {
    fn show(&self, v: f32) {
        if let Some(slider) = self.slider.try_get() {
            slider.set_value(number(v));
        }
        if let Some(field) = self.field.try_get() {
            field.set_value(number(v));
        }
    }
}

/// The widgets that show the brush, to follow it when one of them changes it.
#[derive(Default)]
pub struct BrushShown {
    radius: Pair,
    hardness: Pair,
    channels: [Handle<NumberField<Msg>>; 3],
}

fn edit_msg(edit: BrushEdit) -> Option<Msg> {
    Some(Msg::Tex(TexMsg::Brush(edit)))
}

fn scalar(
    (min, max): (f32, f32),
    value: f32,
    make: fn(f32) -> BrushEdit,
    pair: &Pair,
) -> Layout<Msg> {
    let msg = move |v: f64| edit_msg(make(tidy(v, min, max)));
    let slide = slider(f64::from(min), f64::from(max))
        .then(move |s| {
            s.set_value(number(value));
            s.on_change(msg).on_commit(msg)
        })
        .bind(&pair.slider);
    let step = number((max - min) / 20.0).max(0.001);
    let field = number_field(f64::from(min), f64::from(max), step)
        .then(move |f| {
            f.set_value(number(value));
            f.on_commit(msg)
        })
        .bind(&pair.field);
    row().gap(4).children((slide.fill(1), field.width(84)))
}

fn colour(color: [f32; 3], shown: &BrushShown) -> Layout<Msg> {
    let swatches = color_picker(&SWATCHES)
        .columns(8)
        .on_select(|c: xui::Color| {
            let unit = |v: u8| f32::from(v) / 255.0;
            let rgb = [unit(c.r), unit(c.g), unit(c.b)];
            Msg::Tex(TexMsg::Brush(BrushEdit::Color(rgb)))
        });
    let channel = |i: usize| {
        number_field(0.0, 1.0, 0.05)
            .then(move |f| {
                f.set_value(number(color[i]));
                f.on_commit(move |v| {
                    let mut now = color;
                    now[i] = tidy(v, 0.0, 1.0);
                    edit_msg(BrushEdit::Color(now))
                })
            })
            .bind(&shown.channels[i])
    };
    let fields =
        row()
            .gap(4)
            .children((channel(0).fill(1), channel(1).fill(1), channel(2).fill(1)));
    column().gap(4).children((swatches.height(52), fields))
}

/// The section: radius, hardness and colour of the brush as it is now.
pub fn brush_rows(brush: Brush, shown: &BrushShown) -> Layout<Msg> {
    let range = (*STROKE_RADIUS.start(), *STROKE_RADIUS.end());
    column()
        .gap(6)
        .child(label("Brush").title())
        .child(label("radius"))
        .child(scalar(
            range,
            brush.radius,
            BrushEdit::Radius,
            &shown.radius,
        ))
        .child(label("hardness"))
        .child(scalar(
            (0.0, 1.0),
            brush.hardness,
            BrushEdit::Hardness,
            &shown.hardness,
        ))
        .child(label("colour"))
        .child(colour(brush.color, shown))
}

impl BrushShown {
    /// Shows a change in the widgets that did not make it.
    pub fn show(&self, edit: BrushEdit) {
        match edit {
            BrushEdit::Radius(v) => self.radius.show(v),
            BrushEdit::Hardness(v) => self.hardness.show(v),
            BrushEdit::Color(c) => {
                for (h, v) in self.channels.iter().zip(c) {
                    if let Some(field) = h.try_get() {
                        field.set_value(number(v));
                    }
                }
            }
        }
    }
}
