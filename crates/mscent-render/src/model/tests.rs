use mscent_math::rng::Rng;
use mscent_world::model::{build, read};

use super::fill::ModelFill;
use super::*;
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::material::{Material, MaterialTable};
use crate::mesh::MeshLighting;
use crate::specular::SpecTable;
use crate::texture::Texture;

const SIZE: (u32, u32) = (320, 240);
const BACK: u32 = 0xffff_00ff;
const HEAD: &str = "miniscent-blueprint 1\nname a\nclass drone\nscale 1\n";

/// A material of hard-edged squares, so that any slide of the texture shows.
fn checks() -> Texture {
    let mut pixels = Vec::with_capacity(128 * 128);
    for y in 0..128u32 {
        for x in 0..128u32 {
            let on = (x / 8 + y / 8) % 2 == 0;
            pixels.push(if on { 0xffc0_c0c0 } else { 0xff30_3030 });
        }
    }
    Texture::new(128, 128, pixels).expect("a texture")
}

fn materials() -> MaterialTable {
    let mut table = MaterialTable::new();
    for name in ["checks", "dark"] {
        let texture = if name == "checks" {
            checks()
        } else {
            Texture::new(128, 128, vec![0xff40_4040; 128 * 128]).expect("a texture")
        };
        table.add(Material {
            name: name.into(),
            gloss: 0,
            texture,
        });
    }
    table
}

fn table_of(body: &str) -> (ModelTable, ModelId) {
    let text = format!("{HEAD}skin hull checks\nskin glow dark glow\n{body}");
    let blueprint = read(text.as_bytes()).expect("reads");
    let model = build(&blueprint).expect("builds");
    let mut table = ModelTable::new();
    let id = table.add(&blueprint, &model, &materials());
    (table, id)
}

fn even_light() -> MeshLighting {
    MeshLighting {
        ambient: 0.7,
        sun_dir: Vec3::new(0.0, 0.0, 1.0),
        sun: 0.3,
        points: Vec::new(),
    }
}

fn camera(eye: Vec3, target: Vec3) -> Camera {
    Camera::look_at(
        eye,
        target,
        Vec3::new(0.0, 0.0, 1.0),
        1.0,
        0.1,
        400.0,
        SIZE.0,
        SIZE.1,
    )
    .expect("a camera")
}

/// A frame of `instances`, and the list it made.
fn draw(
    table: &ModelTable,
    instances: &[ModelInstance],
    cam: &Camera,
    lighting: &MeshLighting,
) -> (Framebuffer, ModelList) {
    let mut list = ModelList::default();
    list.prepare(table, instances, cam, lighting, &[], |_| {
        mscent_world::LeafId(0)
    });
    (paint(table, &list, cam, true), list)
}

fn paint(table: &ModelTable, list: &ModelList, cam: &Camera, bump: bool) -> Framebuffer {
    let mats = materials();
    let spec: Vec<SpecTable> = Vec::new();
    let fallback = Texture::new(4, 4, vec![0xff80_8080; 16]).expect("a texture");
    let texture_of = |id: u16| {
        mats.get(crate::material::MaterialId(id))
            .map_or(&fallback, |m| &m.texture)
    };
    let fill = ModelFill {
        cam,
        fog: None,
        bump,
        specular: false,
        spec: &spec,
        texture_of: &texture_of,
    };
    let mut fb = Framebuffer::new(SIZE.0, SIZE.1).expect("fb");
    fb.clear(BACK);
    fill_models(&mut fb, list, table, &fill);
    fb
}

fn drawn(fb: &Framebuffer) -> usize {
    fb.pixels().iter().filter(|&&p| p != BACK).count()
}

const AT_ORIGIN: Quat = Quat::IDENTITY;

#[test]
fn a_model_draws_and_is_textured_by_its_material() {
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let cam = camera(Vec3::new(14.0, -6.0, 8.0), Vec3::ZERO);
    let (fb, list) = draw(
        &table,
        &[ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)],
        &cam,
        &even_light(),
    );
    assert_eq!(list.triangles, 12);
    assert!(drawn(&fb) > 3000, "{} pixels", drawn(&fb));
    let mut shades: Vec<u32> = fb.pixels().iter().copied().filter(|&p| p != BACK).collect();
    shades.sort_unstable();
    shades.dedup();
    assert!(shades.len() > 2, "flat colour, not a texture: {shades:?}");
}

#[test]
fn a_texture_rides_on_a_model_that_moves() {
    // The camera comes along, so the picture must not change. A texture mapped in
    // the world would slide by a fraction of its squares.
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let lighting = MeshLighting {
        sun: 0.0,
        ..even_light()
    };
    let eye = Vec3::new(14.0, -6.0, 8.0);
    let shot = |at: Vec3| {
        let cam = camera(eye + at, at);
        draw(
            &table,
            &[ModelInstance::new(id, at, AT_ORIGIN)],
            &cam,
            &lighting,
        )
        .0
    };
    let still = shot(Vec3::ZERO);
    let moved = shot(Vec3::new(1.37, -2.11, 0.73));
    let differ = |a: &Framebuffer, b: &Framebuffer| {
        a.pixels()
            .iter()
            .zip(b.pixels())
            .filter(|(p, q)| p != q)
            .count()
    };
    let changed = differ(&still, &moved);
    assert!(changed * 50 < drawn(&still), "{changed} pixels changed");
}

#[test]
fn a_texture_rides_on_a_model_that_turns() {
    // Instance and camera turn together about the model's centre.
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let lighting = MeshLighting {
        sun: 0.0,
        ..even_light()
    };
    let axis = Vec3::new(0.0, 0.0, 1.0);
    let eye = Vec3::new(14.0, -6.0, 8.0);
    let shot = |angle: f32| {
        let rot = Quat::from_axis_angle(axis, angle);
        let cam = camera(rot.rotate(eye), Vec3::ZERO);
        draw(
            &table,
            &[ModelInstance::new(id, Vec3::ZERO, rot)],
            &cam,
            &lighting,
        )
        .0
    };
    let (still, turned) = (shot(0.0), shot(0.7));
    let changed = still
        .pixels()
        .iter()
        .zip(turned.pixels())
        .filter(|(p, q)| p != q)
        .count();
    assert!(changed * 25 < drawn(&still), "{changed} pixels changed");
}

#[test]
fn turning_shows_another_side() {
    let (table, id) = table_of("part p\nsolid plate 1  0 0 0  8 0 0  8 0 8 hull\n");
    let cam = camera(Vec3::new(4.0, -20.0, 0.0), Vec3::new(4.0, 0.0, 0.0));
    let place = |angle: f32| {
        let rot = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), angle);
        ModelInstance::new(id, Vec3::ZERO, rot)
    };
    let flat_on = drawn(&draw(&table, &[place(0.0)], &cam, &even_light()).0);
    let edge_on = drawn(
        &draw(
            &table,
            &[place(core::f32::consts::FRAC_PI_2)],
            &cam,
            &even_light(),
        )
        .0,
    );
    assert!(flat_on > edge_on * 2, "{flat_on} against {edge_on}");
}

#[test]
fn a_closed_model_has_no_pinholes_at_any_level_from_any_side() {
    let (table, id) = table_of("part p cell=0.25\nadd sphere 0 0 0 5 hull\n");
    let mut rng = Rng::new(0x51DE);
    for _ in 0..40 {
        let dir = Vec3::new(
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
        );
        let Some(dir) = dir.normalized() else {
            continue;
        };
        let axis = Vec3::new(rng.range_f32(-1.0, 1.0), 1.0, rng.range_f32(-1.0, 1.0));
        let rot = Quat::from_axis_angle(axis, rng.range_f32(0.0, 6.0));
        let distance = rng.range_f32(9.0, 120.0);
        let cam = camera(dir * distance, Vec3::ZERO);
        let (fb, _) = draw(
            &table,
            &[ModelInstance::new(id, Vec3::ZERO, rot)],
            &cam,
            &even_light(),
        );
        // A disc well inside the silhouette, at any distance.
        let focal = cam.focal();
        let inner = 5.0 * focal / distance * 0.8;
        let (cx, cy) = (SIZE.0 as f32 / 2.0, SIZE.1 as f32 / 2.0);
        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy < inner * inner {
                    let p = fb.pixels()[(y * SIZE.0 + x) as usize];
                    assert_ne!(p, BACK, "a hole at {x},{y} from {distance}");
                }
            }
        }
    }
}

#[test]
fn nearer_things_hide_farther_ones_within_a_model() {
    let (table, id) =
        table_of("part a\nsolid cube 0 0 0 2 2 2 hull\npart b\nsolid cube 10 0 0 4 4 4 glow\n");
    let cam = camera(Vec3::new(-20.0, 0.0, 0.0), Vec3::ZERO);
    let (fb, _) = draw(
        &table,
        &[ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)],
        &cam,
        &even_light(),
    );
    let centre = fb.pixels()[(SIZE.1 / 2 * SIZE.0 + SIZE.0 / 2) as usize];
    assert_ne!(centre, BACK);
    let dark = 0xff40_4040u32;
    assert_ne!(centre & 0xff, dark & 0xff, "the far part shows through");
}

#[test]
fn a_glowing_role_stays_bright_with_no_light() {
    let (table, id) =
        table_of("part p\nsolid cube 0 0 0 4 4 4 glow\nsolid cube 0 12 0 4 4 4 hull\n");
    let black = MeshLighting {
        ambient: 0.0,
        sun_dir: Vec3::new(0.0, 0.0, 1.0),
        sun: 0.0,
        points: Vec::new(),
    };
    let cam = camera(Vec3::new(25.0, 6.0, 0.0), Vec3::new(0.0, 6.0, 0.0));
    let (fb, _) = draw(
        &table,
        &[ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)],
        &cam,
        &black,
    );
    let lit: Vec<u32> = fb.pixels().iter().copied().filter(|&p| p != BACK).collect();
    let bright = lit.iter().filter(|&&p| p & 0xff > 0x20).count();
    let black_pixels = lit.iter().filter(|&&p| p & 0xff_ffff == 0).count();
    assert!(bright > 500, "{bright} bright pixels");
    assert!(black_pixels > 500, "{black_pixels} of the unlit cube");
}

#[test]
fn a_tint_multiplies_the_light() {
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let cam = camera(Vec3::new(14.0, -6.0, 8.0), Vec3::ZERO);
    let mut red = ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN);
    red.tint = Vec3::new(1.0, 0.0, 0.0);
    let (fb, _) = draw(&table, &[red], &cam, &even_light());
    let lit = fb.pixels().iter().filter(|&&p| p != BACK);
    assert!(lit.clone().count() > 1000);
    assert!(lit.clone().all(|p| p & 0xff == 0), "no blue");
    assert!(lit.clone().any(|p| (p >> 16) & 0xff > 0x20), "some red");
}

#[test]
fn a_model_crossing_the_near_plane_draws_without_trouble() {
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let cam = camera(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.2, 0.1));
    // The eye is inside a cube: its inner sides face away, so nothing shows.
    let inside = draw(
        &table,
        &[ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)],
        &cam,
        &even_light(),
    );
    assert_eq!(drawn(&inside.0), 0);
    // Half past the near plane, the near end cut off.
    let beside = camera(Vec3::new(4.05, 0.0, 0.0), Vec3::new(4.05, 8.0, 0.0));
    let (fb, _) = draw(
        &table,
        &[ModelInstance::new(id, Vec3::new(0.0, 4.0, 0.0), AT_ORIGIN)],
        &beside,
        &even_light(),
    );
    assert!(drawn(&fb) > 0);
}

#[test]
fn a_far_model_takes_a_lower_level_and_comes_back() {
    let (table, id) =
        table_of("part p\nsolid cube 0 0 0 4 4 4 hull\nsolid cube 8 0 0 1 1 1 hull detail\n");
    let instance = [ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)];
    let mut list = ModelList::default();
    let level = |list: &mut ModelList, distance: f32| {
        let cam = camera(Vec3::new(0.0, -distance, 0.0), Vec3::ZERO);
        list.prepare(&table, &instance, &cam, &even_light(), &[], |_| {
            mscent_world::LeafId(0)
        });
        list.parts[0].lod
    };
    assert_eq!(level(&mut list, 12.0), 0);
    assert_eq!(list.triangles, 24);
    let far = level(&mut list, 200.0);
    assert!(far > 0);
    assert_eq!(list.triangles, 12, "the detail piece is gone");
    assert_eq!(level(&mut list, 12.0), 0);
}

#[test]
fn past_the_budget_the_farthest_take_the_lowest_level() {
    let (table, id) = table_of("part p cell=0.25\nadd sphere 0 0 0 5 hull\n");
    let full = table.get(id.0).expect("model").parts[0][0].triangles.len() as u32;
    let lowest = table.get(id.0).expect("model").parts[0][2].triangles.len() as u32;
    assert!(lowest < full);
    let count = (MAX_MODEL_TRIS / full) as usize + 8;
    let instances: Vec<ModelInstance> = (0..count)
        .map(|i| ModelInstance::new(id, Vec3::new(0.0, 12.0 + i as f32 * 0.01, 0.0), AT_ORIGIN))
        .collect();
    let cam = camera(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0));
    let mut list = ModelList::default();
    list.prepare(&table, &instances, &cam, &even_light(), &[], |_| {
        mscent_world::LeafId(0)
    });
    let nearest = list.parts.first().expect("a part");
    let farthest = list.parts.last().expect("a part");
    assert_eq!(nearest.lod, 0);
    assert_eq!(farthest.lod, 2);
    assert_eq!(list.parts.len(), count, "nothing is left out");
    assert!(list.triangles < count as u32 * full);
}

#[test]
fn what_cannot_be_drawn_is_left_out() {
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let cam = camera(Vec3::new(14.0, -6.0, 8.0), Vec3::ZERO);
    let good = ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN);
    let nan = f32::NAN;
    let bad = [
        ModelInstance::new(ModelId(99), Vec3::ZERO, AT_ORIGIN),
        ModelInstance {
            pos: Vec3::new(nan, 0.0, 0.0),
            ..good
        },
        ModelInstance {
            rot: Quat {
                x: nan,
                ..AT_ORIGIN
            },
            ..good
        },
        ModelInstance {
            rot: Quat {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            ..good
        },
        ModelInstance { scale: 0.0, ..good },
        ModelInstance { scale: nan, ..good },
        ModelInstance {
            tint: Vec3::new(nan, 1.0, 1.0),
            ..good
        },
    ];
    for instance in bad {
        let (fb, list) = draw(&table, &[instance], &cam, &even_light());
        assert_eq!(drawn(&fb), 0, "{instance:?}");
        assert!(list.parts.is_empty());
    }
}

#[test]
fn a_model_behind_the_eye_submits_nothing() {
    let (table, id) = table_of("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let cam = camera(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0));
    let behind = ModelInstance::new(id, Vec3::new(0.0, -50.0, 0.0), AT_ORIGIN);
    let (_, list) = draw(&table, &[behind], &cam, &even_light());
    assert_eq!(list.triangles, 0);
}

#[test]
fn an_unknown_material_draws_grey_and_does_not_fail() {
    let text = format!("{HEAD}skin hull nothing\npart p\nsolid cube 0 0 0 4 4 4 hull\n");
    let blueprint = read(text.as_bytes()).expect("reads");
    let model = build(&blueprint).expect("builds");
    let mut table = ModelTable::new();
    let id = table.add(&blueprint, &model, &materials());
    let cam = camera(Vec3::new(14.0, -6.0, 8.0), Vec3::ZERO);
    let (fb, _) = draw(
        &table,
        &[ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)],
        &cam,
        &even_light(),
    );
    assert!(drawn(&fb) > 1000);
}

#[test]
fn every_stock_blueprint_draws() {
    for text in [
        include_str!("../../../../assets/enemies/sentry.msb"),
        include_str!("../../../../assets/enemies/dart.msb"),
        include_str!("../../../../assets/enemies/mantis.msb"),
    ] {
        let blueprint = read(text.as_bytes()).expect("reads");
        let model = build(&blueprint).expect("builds");
        let mut table = ModelTable::new();
        let id = table.add(&blueprint, &model, &materials());
        let cam = camera(Vec3::new(5.0, -5.0, 2.0), Vec3::ZERO);
        let (fb, list) = draw(
            &table,
            &[ModelInstance::new(id, Vec3::ZERO, AT_ORIGIN)],
            &cam,
            &even_light(),
        );
        assert!(drawn(&fb) > 300, "{}: {}", blueprint.name, drawn(&fb));
        assert!(list.triangles > 100);
        // Bands need the same picture as a whole frame, so a list holds a row span
        // for every part it has.
        assert_eq!(list.parts.len(), blueprint.parts.len());
    }
}
