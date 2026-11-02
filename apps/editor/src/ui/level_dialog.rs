//! The level settings window, whose OK creates or updates the level's world
//! entity. Fog lives here rather than in a placeable entity because a level has
//! exactly one, and it has no place in the map. It is an ordinary window and not a
//! modal one: the default backend has no modal loop, and `open_modal` returns at
//! once having shown nothing.

use mscent_world::entity::MAX_FOG_DEPTH;
use xui::app::WindowHandle;
use xui::backend::PlatformSpec;
use xui::dip;
use xui::prelude::*;
use xui::widget::HasText;

use super::app::Msg;
use crate::model::entity::fog_entity;

#[derive(Clone)]
pub enum FogMsg {
    Ok,
    Cancel,
}

/// The window, if it is up.
pub type FogWindow = WindowHandle<FogMsg>;

struct LevelWindow {
    /// Where the answer goes.
    editor: Ui<Msg>,
    start: Handle<NumberField<FogMsg>>,
    end: Handle<NumberField<FogMsg>>,
    problem: Handle<Label<FogMsg>>,
}

/// The range as the world entity takes it, or what to say about it.
fn checked_fog(start: f64, end: f64) -> std::result::Result<(f32, f32), String> {
    let fog = (start as f32, end as f32);
    if fog_entity(fog.0, fog.1).is_valid() {
        Ok(fog)
    } else {
        Err(format!(
            "Start must be below end, and end at most {MAX_FOG_DEPTH}."
        ))
    }
}

impl App for LevelWindow {
    type Msg = FogMsg;

    fn update(&mut self, msg: FogMsg, ui: &mut Ui<FogMsg>) {
        match msg {
            FogMsg::Ok => match checked_fog(self.start.get().value(), self.end.get().value()) {
                Ok((start, end)) => {
                    self.editor.emit(Msg::SetFog(start, end));
                    ui.close();
                }
                // Stays open on a bad range so the typed values are not lost.
                Err(why) => self.problem.get().set_text(&why),
            },
            FogMsg::Cancel => ui.close(),
        }
    }
}

fn fog_field(handle: &Handle<NumberField<FogMsg>>) -> Build<NumberField<FogMsg>, FogMsg> {
    number_field(0.0, f64::from(MAX_FOG_DEPTH), 1.0).bind(handle)
}

/// Opens the window on the fog range `current`.
pub fn open(ui: &Ui<Msg>, current: (f32, f32)) -> xui::backend::Result<FogWindow> {
    let spec = PlatformSpec::new("Level settings").size(dip(320.0), dip(180.0));
    let editor = ui.clone();
    ui.open_window(spec, move |ui| {
        let window = LevelWindow {
            editor,
            start: Handle::new(),
            end: Handle::new(),
            problem: Handle::new(),
        };
        let fields = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Fog start"))
            .child(fog_field(&window.start))
            .child(label("Fog end"))
            .child(fog_field(&window.end));
        let buttons = row().gap(6).children((
            button("OK").on_click(FogMsg::Ok),
            button("Cancel").on_click(FogMsg::Cancel),
        ));
        let root = column().padding(12).gap(10).children((
            group("Fog", column().padding(6).child(fields)),
            label("").bind(&window.problem),
            buttons,
        ));
        if ui.root(root).is_err() {
            ui.close();
        }
        window.start.get().set_value(f64::from(current.0));
        window.end.get().set_value(f64::from(current.1));
        window
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_the_world_entity_takes_goes_through_and_any_other_is_explained() {
        assert_eq!(checked_fog(4.0, 26.0), Ok((4.0, 26.0)));
        assert!(checked_fog(26.0, 4.0).is_err(), "start past end");
        assert!(checked_fog(4.0, 4.0).is_err(), "no range");
        let too_far = f64::from(MAX_FOG_DEPTH) + 1.0;
        assert!(checked_fog(4.0, too_far).unwrap_err().contains("at most"));
        assert!(checked_fog(f64::NAN, 26.0).is_err());
    }
}
