//! Enemy models for the parity cases: a probe skinned in stock materials, as the
//! skins of `assets/enemies` are not in `assets/textures` yet, so that a case sees
//! textures, bump and highlight on it rather than the grey of an unknown material.

use std::path::Path;
use std::sync::OnceLock;

use mscent_math::{Quat, Vec3};
use mscent_render::MaterialTable;
use mscent_render::View;
use mscent_render::model::{ModelId, ModelInstance, ModelTable};
use mscent_world::model::{build, read};

/// Faceted solids in metal and brick, with a cut, a voxel ball where the
/// projections blend, a glowing lens, and a metal dome, whose every normal
/// catches a highlight from some light.
const PROBE: &str = "miniscent-blueprint 1
name probe
class drone
scale 0.25
skin hull metal
skin trim brick
skin lens lightpanel glow

part hub cell=0.5
solid ngon -2.5 0 0  0 0 0  2.5 4.25  7 hull
solid ngon 0 0 0  2 0 0  4.25 2.75  7 trim
add sphere 2 0 0 1.75 lens
cut ngon 3.5 0 0  6 0 0  0.5 1  6 hull

part pod cell=0.25
add sphere 0 0 4.5 2 trim

part dome cell=0.25
add sphere -1 0 -4 2 hull
";

/// The probe, built once. Material ids are the stock ones, the same in every
/// table a case uses.
pub fn table() -> &'static (ModelTable, ModelId) {
    static TABLE: OnceLock<(ModelTable, ModelId)> = OnceLock::new();
    TABLE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        let materials = MaterialTable::load_dir(&dir).expect("textures");
        let blueprint = read(PROBE.as_bytes()).expect("the probe reads");
        let model = build(&blueprint).expect("the probe builds");
        let mut table = ModelTable::new();
        let id = table.add(&blueprint, &model, &materials);
        (table, id)
    })
}

/// Three probes `ahead` units in front of `view`: one square on, two turned so
/// that their sides catch the light, and the right one larger.
pub fn ahead_of(view: &View, ahead: f32) -> Vec<ModelInstance> {
    let id = table().1;
    let forward = view.forward();
    let across = Vec3::new(forward.y, -forward.x, 0.0);
    let up = Vec3::new(0.0, 0.0, 1.0);
    let centre = view.pos + forward * ahead;
    let spots = [
        (centre, 0.0, 1.0),
        (centre - across * (0.45 * ahead), 0.9, 1.0),
        (centre + across * (0.45 * ahead) + up * 0.3, -2.1, 1.4),
    ];
    spots
        .into_iter()
        .map(|(pos, turn, scale)| {
            let tilt = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), 0.4);
            let rot = Quat::from_axis_angle(up, turn) * tilt;
            ModelInstance {
                scale,
                ..ModelInstance::new(id, pos, rot)
            }
        })
        .collect()
}
