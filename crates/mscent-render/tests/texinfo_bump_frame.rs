//! Review check: the frame that turns baked light directions into the normal
//! map's frame must follow the face's TexInfo exactly, including mirrored
//! (negative) scales and rotation. Expected values come from an independent
//! oracle built from the TexInfo axes, not from the code under test.

use mscent_math::{Vec3, Vec3d};
use mscent_render::{Texture, UvToLuxel, Vertex};
use mscent_world::lightmap::LightFace;
use mscent_world::lightmap::uv::project_face;
use mscent_world::texinfo::TexInfo;

fn check(info: TexInfo, normal: Vec3d, quad: [Vec3; 4]) {
    let tex = Texture::new(128, 128, vec![0xff80_8080; 128 * 128]).expect("tex");
    let face = LightFace {
        vertices: quad.to_vec(),
        normal: normal.to_f32(),
        density: Default::default(),
    };
    let proj = project_face(&face, 0, 4.0).expect("projection");
    let verts: Vec<Vertex> = quad
        .iter()
        .map(|p| {
            let (u, v) = info.uv_at(p.to_f64(), (128, 128));
            Vertex::new(*p, u, v)
        })
        .collect();
    let map = UvToLuxel::from_vertices(&proj, &verts, &tex).expect("map");
    let (u, v) = info.effective_axes();
    let (su, sv) = (
        f64::from(info.u_scale).signum(),
        f64::from(info.v_scale).signum(),
    );
    // Texture +u in the world is U * sign(u_scale); the image's up is -v.
    let right = u * su;
    let up = -(v * sv);
    for dir in [
        Vec3::new(0.6, 0.0, 0.8),
        Vec3::new(0.0, 0.6, 0.8),
        Vec3::new(-0.48, 0.36, 0.8),
    ] {
        // A world direction on the face plane, expressed in the lightmap frame.
        let world = proj.tangent * dir.x + proj.bitangent * dir.y + proj.normal * dir.z;
        let got = map.to_texture_frame(dir);
        let w = world.to_f64();
        let want = (w.dot(right) as f32, w.dot(up) as f32);
        assert!(
            (got.x - want.0).abs() < 1e-3 && (got.y - want.1).abs() < 1e-3,
            "info {info:?}: got ({}, {}) want {want:?}",
            got.x,
            got.y
        );
    }
}

fn floor_quad() -> [Vec3; 4] {
    [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(40.0, 0.0, 0.0),
        Vec3::new(40.0, 30.0, 0.0),
        Vec3::new(0.0, 30.0, 0.0),
    ]
}

fn wall_quad() -> [Vec3; 4] {
    [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 40.0, 0.0),
        Vec3::new(0.0, 40.0, 30.0),
        Vec3::new(0.0, 0.0, 30.0),
    ]
}

#[test]
fn bump_frame_follows_flips_and_rotation_on_floor_and_wall() {
    for (normal, quad) in [
        (Vec3d::new(0.0, 0.0, 1.0), floor_quad()),
        (Vec3d::new(1.0, 0.0, 0.0), wall_quad()),
        (Vec3d::new(-1.0, 0.0, 0.0), wall_quad()),
    ] {
        for (fu, fv, rot) in [
            (false, false, 0.0),
            (true, false, 0.0),
            (false, true, 0.0),
            (true, true, 0.0),
            (false, false, 33.0),
            (true, false, 33.0),
            (true, true, -120.0),
        ] {
            let mut info = TexInfo::default_for_normal(normal);
            if fu {
                info.flip_u();
            }
            if fv {
                info.flip_v();
            }
            info.rotate(rot);
            check(info, normal, quad);
        }
    }
}
