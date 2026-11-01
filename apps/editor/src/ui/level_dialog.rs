//! The level settings dialog: a modal window whose OK creates or updates the
//! level's world entity. Fog lives here rather than in a placeable entity
//! because a level has exactly one, and it has no place in the map.

use mscent_world::entity::MAX_FOG_DEPTH;
use xui::Dip;
use xui::backend::PlatformSpec;
use xui::prelude::*;

use super::app::Msg;

#[derive(Clone)]
enum DialogMsg {
    Ok,
    Cancel,
}

struct LevelDialog {
    start: Handle<NumberField<DialogMsg>>,
    end: Handle<NumberField<DialogMsg>>,
}

impl App for LevelDialog {
    type Msg = DialogMsg;

    fn update(&mut self, msg: DialogMsg, ui: &mut Ui<DialogMsg>) {
        match msg {
            DialogMsg::Ok => {
                let fog = (
                    self.start.get().value() as f32,
                    self.end.get().value() as f32,
                );
                ui.close_with_result(fog);
            }
            DialogMsg::Cancel => ui.close(),
        }
    }
}

fn fog_field(handle: &Handle<NumberField<DialogMsg>>) -> Build<NumberField<DialogMsg>, DialogMsg> {
    number_field(0.0, f64::from(MAX_FOG_DEPTH), 1.0).bind(handle)
}

/// Asks for the fog range, starting from `current`; `None` if cancelled.
pub fn ask_fog(ui: &Ui<Msg>, current: (f32, f32)) -> Option<(f32, f32)> {
    let spec = PlatformSpec::new("Level settings").size(Dip(320.0), Dip(180.0));
    ui.open_modal(spec, move |ui| {
        let dialog = LevelDialog {
            start: Handle::new(),
            end: Handle::new(),
        };
        let fields = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Fog start"))
            .child(fog_field(&dialog.start))
            .child(label("Fog end"))
            .child(fog_field(&dialog.end));
        let buttons = row().gap(6).children((
            button("OK").on_click(DialogMsg::Ok),
            button("Cancel").on_click(DialogMsg::Cancel),
        ));
        let root = column()
            .padding(12)
            .gap(10)
            .children((group("Fog", column().padding(6).child(fields)), buttons));
        if ui.root(root).is_err() {
            ui.close();
        }
        dialog.start.get().set_value(f64::from(current.0));
        dialog.end.get().set_value(f64::from(current.1));
        dialog
    })
}
