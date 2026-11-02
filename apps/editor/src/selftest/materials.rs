//! Selftest of replacing the material table while the editor runs: the picture
//! and the panels follow with nothing but a new generation in the store.

use mscent_render::{Material, MaterialTable, Texture};
use mscent_world::materials::NAMES;
use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump, wait_for_preview};
use crate::studio::Preview;
use crate::ui::{Msg, Probe};

const PAINT: u32 = 0xff20_c040;

/// The stock names in one flat colour, and one material past the stock ids.
fn painted() -> Result<MaterialTable, String> {
    let mut table = MaterialTable::new();
    for name in NAMES.into_iter().chain(["painted"]) {
        let texture = Texture::new(128, 128, vec![PAINT; 128 * 128]).map_err(|e| e.to_string())?;
        table.add(Material {
            name: name.into(),
            gloss: 0,
            texture,
        });
    }
    Ok(table)
}

pub(super) fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    let table = match painted() {
        Ok(table) => table,
        Err(why) => return run.check(false, &format!("a table to install: {why}")),
    };
    let pane = probe.view_pane();
    let before = pane.frame_checksum();
    let (sizes, generation) = (
        probe.state().borrow().texture_sizes.len(),
        probe.materials().replace(table),
    );
    run.check(generation == 1, &format!("a new generation: {generation}"));
    let p = wait_for_preview(stage, probe);
    run.check(p == Preview::Full, &format!("full light kept: {p:?}"));
    pump(stage, 30);
    run.check(
        pane.frame_checksum() != before,
        "the new materials reached the picture",
    );
    let now = probe.state().borrow().texture_sizes.len();
    run.check(now == sizes + 1, &format!("sizes {sizes} became {now}"));
    run.shot(stage, "wide-04-materials.png");
}
