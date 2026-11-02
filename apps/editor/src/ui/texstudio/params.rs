//! The parameters panel: generated from the selected operator's table, so an
//! operator needs no UI code of its own. A combo box cannot change its items, so
//! the whole panel is remade in its container when the selection changes, as the
//! material lists are; while a value is being dragged it is left alone.

use mscent_texgen::ops::{OpSpec, ParamKind, ParamSpec, Stroke, Value};
use xui::Color;
use xui::backend::{Result as XuiResult, WidgetId};
use xui::prelude::*;

use super::TexMsg;
use crate::model::texdoc::TexDoc;
use crate::ui::Msg;

/// The container the widgets are remade in.
pub type Host = xui::widget::Panel<Msg>;

/// A range this wide or wider is a number to type, not a thing to slide.
const SLIDE_RANGE: i64 = 1024;

/// A starting point for a colour; the fields below it say exactly which.
const SWATCHES: [Color; 16] = [
    Color::rgb(0, 0, 0),
    Color::rgb(64, 64, 64),
    Color::rgb(128, 128, 128),
    Color::rgb(192, 192, 192),
    Color::rgb(255, 255, 255),
    Color::rgb(140, 56, 38),
    Color::rgb(186, 96, 58),
    Color::rgb(214, 170, 96),
    Color::rgb(110, 92, 70),
    Color::rgb(70, 92, 60),
    Color::rgb(120, 150, 90),
    Color::rgb(60, 100, 140),
    Color::rgb(120, 160, 200),
    Color::rgb(90, 70, 130),
    Color::rgb(200, 60, 60),
    Color::rgb(230, 200, 60),
];

/// A value as a recipe should hold it: three decimals, inside its range.
fn tidy(v: f64, min: f32, max: f32) -> f32 {
    (((v * 1000.0).round() / 1000.0) as f32).clamp(min, max)
}

fn number(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

fn param_msg(block: usize, index: usize, value: Value, live: bool) -> Msg {
    Msg::Tex(TexMsg::Param {
        block,
        index,
        value,
        live,
    })
}

/// The widgets of one parameter that show its number.
#[derive(Default)]
struct Shown {
    slider: Option<Handle<Slider<Msg>>>,
    field: Option<Handle<NumberField<Msg>>>,
    channels: [Handle<NumberField<Msg>>; 3],
    text: Option<Handle<Edit<Msg>>>,
    combo: Option<Handle<ComboBox<Msg>>>,
}

fn float_row(
    (block, index): (usize, usize),
    (min, max): (f32, f32),
    value: f32,
    shown: &mut Shown,
) -> Layout<Msg> {
    let (slider_h, field_h) = (Handle::new(), Handle::new());
    let msg = move |v: f64, live| {
        Some(param_msg(
            block,
            index,
            Value::Float(tidy(v, min, max)),
            live,
        ))
    };
    let slide = slider(f64::from(min), f64::from(max))
        .then(move |s| {
            s.set_value(number(value));
            s.on_change(move |v| msg(v, true))
                .on_commit(move |v| msg(v, false))
        })
        .bind(&slider_h);
    let step = number((max - min) / 20.0).max(0.001);
    let field = number_field(f64::from(min), f64::from(max), step)
        .then(move |f| {
            f.set_value(number(value));
            f.on_commit(move |v| msg(v, false))
        })
        .bind(&field_h);
    shown.slider = Some(slider_h);
    shown.field = Some(field_h);
    row().gap(4).children((slide.fill(1), field.width(84)))
}

fn int_row(
    (block, index): (usize, usize),
    (min, max): (i32, i32),
    value: i32,
    shown: &mut Shown,
) -> Layout<Msg> {
    let msg = move |v: f64, live| {
        let v = (v.round() as i64).clamp(i64::from(min), i64::from(max)) as i32;
        Some(param_msg(block, index, Value::Int(v), live))
    };
    let field_h = Handle::new();
    let field = number_field(f64::from(min), f64::from(max), 1.0)
        .then(move |f| {
            f.set_value(f64::from(value));
            f.on_commit(move |v| msg(v, false))
        })
        .bind(&field_h);
    shown.field = Some(field_h);
    if i64::from(max) - i64::from(min) >= SLIDE_RANGE {
        return row().child(field.fill(1));
    }
    let slider_h = Handle::new();
    let slide = slider(f64::from(min), f64::from(max))
        .then(move |s| {
            s.set_value(f64::from(value));
            s.on_change(move |v| msg(v, true))
                .on_commit(move |v| msg(v, false))
        })
        .bind(&slider_h);
    shown.slider = Some(slider_h);
    row().gap(4).children((slide.fill(1), field.width(84)))
}

fn choice_row(
    (block, index): (usize, usize),
    names: &[&str],
    value: u8,
    shown: &mut Shown,
) -> Layout<Msg> {
    let handle = Handle::new();
    let list = combo_box(names)
        .on_select(move |row| {
            let row = u8::try_from(row).unwrap_or(u8::MAX);
            param_msg(block, index, Value::Choice(row), false)
        })
        .then(move |c| {
            c.select(usize::from(value));
            c
        })
        .bind(&handle);
    shown.combo = Some(handle);
    column().child(list)
}

fn colour_rows((block, index): (usize, usize), value: [f32; 4], shown: &mut Shown) -> Layout<Msg> {
    let alpha = value[3];
    let swatches = color_picker(&SWATCHES)
        .columns(8)
        .on_select(move |c: Color| {
            let unit = |v: u8| f32::from(v) / 255.0;
            let value = Value::Color([unit(c.r), unit(c.g), unit(c.b), alpha]);
            param_msg(block, index, value, false)
        });
    let channel = |i: usize, handle: &Handle<NumberField<Msg>>| {
        number_field(0.0, 1.0, 0.05)
            .then(move |f| {
                f.set_value(number(value[i]));
                f.on_commit(move |v| {
                    Some(Msg::Tex(TexMsg::Channel {
                        block,
                        index,
                        channel: i,
                        value: tidy(v, 0.0, 1.0),
                    }))
                })
            })
            .bind(handle)
    };
    let fields = row().gap(4).children((
        channel(0, &shown.channels[0]).fill(1),
        channel(1, &shown.channels[1]).fill(1),
        channel(2, &shown.channels[2]).fill(1),
    ));
    column().gap(4).children((swatches.height(52), fields))
}

fn text_row((block, index): (usize, usize), value: &str, shown: &mut Shown) -> Layout<Msg> {
    let handle = Handle::new();
    let field = edit()
        .text(value)
        .on_change(move |t| param_msg(block, index, Value::Text(t), true))
        .bind(&handle);
    shown.text = Some(handle);
    column().child(field)
}

fn strokes_row((block, index): (usize, usize), strokes: &[Stroke]) -> Layout<Msg> {
    let text = match strokes.len() {
        1 => "1 stroke".to_string(),
        n => format!("{n} strokes"),
    };
    let clear = button("Clear").on_click(Msg::Tex(TexMsg::ClearStrokes { block, index }));
    row()
        .gap(6)
        .children((label(text).align_y(Align::Center).fill(1), clear))
}

fn row_of(at: (usize, usize), param: &ParamSpec, value: &Value, shown: &mut Shown) -> Layout<Msg> {
    match (param.kind, value) {
        (ParamKind::Float { min, max, .. }, Value::Float(v)) => {
            float_row(at, (min, max), *v, shown)
        }
        (ParamKind::Int { min, max, .. }, Value::Int(v)) => int_row(at, (min, max), *v, shown),
        (ParamKind::Choice { names, .. }, Value::Choice(v)) => choice_row(at, names, *v, shown),
        (ParamKind::Color { .. }, Value::Color(c)) => colour_rows(at, *c, shown),
        (ParamKind::Text { .. }, Value::Text(t)) => text_row(at, t, shown),
        (ParamKind::Strokes, Value::Strokes(s)) => strokes_row(at, s),
        // A value of another kind cannot be built from the table.
        _ => row().child(label("?")),
    }
}

/// The handles and how to know when they are out of date.
#[derive(Default)]
pub struct ParamPanel {
    ui: Option<Ui<Msg>>,
    pub host: Handle<Host>,
    /// The material and panel epoch they show.
    shown_of: Option<(String, u64)>,
    shown: Vec<Shown>,
}

/// What the panel holds with no block to show.
pub(super) fn nothing_selected() -> Layout<Msg> {
    column().child(label("Select a block on the page"))
}

fn panel_layout(doc: &TexDoc, shown: &mut Vec<Shown>) -> Layout<Msg> {
    let selected = doc.selected();
    let block = selected.and_then(|b| doc.recipe().blocks.get(b));
    let (Some(index), Some(block)) = (selected, block) else {
        return nothing_selected();
    };
    let spec: &OpSpec = block.spec;
    let mut content = column().gap(6).child(label(spec.name).title());
    if spec.params.is_empty() {
        content = content.child(label("No parameters"));
    }
    for (i, (param, value)) in spec.params.iter().zip(block.params.values()).enumerate() {
        let mut one = Shown::default();
        let entry = row_of((index, i), param, value, &mut one);
        shown.push(one);
        content = content.child(label(param.name)).child(entry);
    }
    content
}

impl ParamPanel {
    pub fn attach(&mut self, ui: &Ui<Msg>) {
        self.ui = Some(ui.clone());
    }

    /// Whether `id` takes typed keys, so shortcuts leave it alone.
    pub fn owns(&self, id: WidgetId) -> bool {
        self.shown.iter().any(|s| {
            let number = |h: &Handle<NumberField<Msg>>| h.try_get().is_some_and(|f| f.id() == id);
            s.field.as_ref().is_some_and(number)
                || s.channels.iter().any(number)
                || s.text
                    .as_ref()
                    .and_then(Handle::try_get)
                    .is_some_and(|e| e.id() == id)
                || s.combo
                    .as_ref()
                    .and_then(Handle::try_get)
                    .is_some_and(|c| c.id() == id)
        })
    }

    /// Makes the widgets show `doc`, if they are not showing it.
    pub fn sync(&mut self, doc: Option<&TexDoc>) -> XuiResult<()> {
        let key = doc.map(|d| (d.name().to_string(), d.panel_epoch()));
        if key == self.shown_of {
            return Ok(());
        }
        let Some(host) = self.host.try_get() else {
            return Ok(());
        };
        let mut shown = Vec::new();
        let layout = match doc {
            Some(doc) => panel_layout(doc, &mut shown),
            None => nothing_selected(),
        };
        host.set_layout(layout)?;
        // After the new widgets exist: asked before, the next pass would measure
        // the old ones and leave the new ones unplaced until something else moved.
        if let Some(ui) = &self.ui {
            ui.invalidate_layout();
        }
        self.shown = shown;
        self.shown_of = key;
        Ok(())
    }

    /// Shows a value in the widgets of its parameter that did not set it: a slider
    /// moving updates the number, and a number typed moves the slider.
    pub fn show_value(&self, index: usize, value: &Value) {
        let Some(shown) = self.shown.get(index) else {
            return;
        };
        let number_of = match value {
            Value::Float(v) => Some(number(*v)),
            Value::Int(v) => Some(f64::from(*v)),
            _ => None,
        };
        if let Some(v) = number_of {
            if let Some(slider) = shown.slider.as_ref().and_then(Handle::try_get) {
                slider.set_value(v);
            }
            if let Some(field) = shown.field.as_ref().and_then(Handle::try_get) {
                field.set_value(v);
            }
        }
        if let Value::Color(c) = value {
            for (h, v) in shown.channels.iter().zip(c) {
                if let Some(field) = h.try_get() {
                    field.set_value(number(*v));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_values_stay_in_range_and_short() {
        assert_eq!(tidy(0.123_456, 0.0, 1.0), 0.123);
        assert_eq!(tidy(1.9, 0.0, 1.0), 1.0);
        assert_eq!(tidy(-1.0, 0.0, 1.0), 0.0);
        assert!(tidy(f64::NAN, 0.0, 1.0).is_nan(), "the table refuses it");
    }
}
