use super::*;
use mscent_math::Polygon;

const TEX: (u32, u32) = (128, 128);

fn p(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn poly(v: &[Vec3d]) -> Polygon {
    Polygon::new(v.to_vec()).expect("test polygon")
}

/// Floor quad on z = 0, counter-clockwise seen from above.
fn floor(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
    poly(&[
        p(x0, y0, 0.0),
        p(x1, y0, 0.0),
        p(x1, y1, 0.0),
        p(x0, y1, 0.0),
    ])
}

fn close(a: (f32, f32), b: (f32, f32)) {
    assert!(
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4,
        "{a:?} != {b:?}"
    );
}

/// Distance of `x` to the nearest whole number.
fn off_integer(x: f32) -> f32 {
    (x - x.round()).abs()
}

/// The projection the renderer used before TexInfo existed.
fn old_uv(pos: Vec3d, n: Vec3d) -> (f32, f32) {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    let axes = if az >= ax && az >= ay {
        (0, 1)
    } else if ax >= ay {
        (1, 2)
    } else {
        (0, 2)
    };
    let q = pos.to_f32();
    let pick = |a: usize| [q.x, q.y, q.z][a] / 8.0;
    (pick(axes.0), pick(axes.1))
}

#[test]
fn default_mapping_reproduces_the_old_projection() {
    let normals = [
        p(0.0, 0.0, 1.0),
        p(0.0, 0.0, -1.0),
        p(1.0, 0.0, 0.0),
        p(-1.0, 0.0, 0.0),
        p(0.0, 1.0, 0.0),
        p(0.0, -1.0, 0.0),
        p(-0.35, 0.0, 0.94),
        p(0.6, 0.6, 0.5),
        p(0.5, 0.5, 0.5),
        p(0.7, -0.7, 0.0),
    ];
    let points = [p(0.0, 0.0, 0.0), p(3.5, -7.25, 12.0), p(-40.0, 33.0, 5.5)];
    for n in normals {
        let t = TexInfo::default_for_normal(n);
        for q in points {
            close(t.uv_at(q, TEX), old_uv(q, n));
        }
    }
}

#[test]
fn scale_sets_the_tile_size_in_world_units() {
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    t.set_scale(0.25, 0.5);
    // One tile is 128 * 0.25 = 32 units along u and 64 along v.
    close(t.uv_at(p(32.0, 0.0, 0.0), TEX), (1.0, 0.0));
    close(t.uv_at(p(0.0, 64.0, 0.0), TEX), (0.0, 1.0));
    // A 64 texel texture covers half as much world per tile.
    close(t.uv_at(p(32.0, 0.0, 0.0), (64, 64)), (2.0, 0.0));
}

#[test]
fn shift_is_in_texels() {
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    t.shift_by(32.0, -64.0);
    close(t.uv_at(p(0.0, 0.0, 0.0), TEX), (0.25, -0.5));
}

#[test]
fn rotation_turns_the_image_counter_clockwise() {
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    t.rotate(90.0);
    // Image right (formerly +x) now points along +y.
    close(t.uv_at(p(0.0, 8.0, 0.0), TEX), (1.0, 0.0));
    close(t.uv_at(p(8.0, 0.0, 0.0), TEX), (0.0, -1.0));
    t.rotate(270.0);
    assert!(t.rotation_deg.abs() < 1e-3, "wrapped back to 0");
}

#[test]
fn flip_mirrors_the_image() {
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    t.shift_by(10.0, 0.0);
    let before = t.uv_at(p(2.0, 3.0, 0.0), TEX);
    t.flip_u();
    let after = t.uv_at(p(2.0, 3.0, 0.0), TEX);
    close(after, (-before.0, before.1));
    t.flip_v();
    assert!(t.u_scale < 0.0 && t.v_scale < 0.0);
}

#[test]
fn hostile_values_never_panic_and_are_repaired() {
    let n = p(1.0, 0.0, 0.0);
    let junk = TexInfo {
        material: 9,
        u_axis: Vec3::new(f32::NAN, 0.0, 0.0),
        v_axis: Vec3::new(0.0, 0.0, 0.0),
        u_shift: f32::INFINITY,
        v_shift: 1.0e30,
        u_scale: 0.0,
        v_scale: 1.0e30,
        rotation_deg: f32::NAN,
        lock_to_world: false,
    };
    assert!(!junk.is_valid());
    let fixed = junk.sanitized(n);
    assert!(fixed.is_valid(), "{fixed:?}");
    assert_eq!(fixed.material, 9);
    assert_eq!(fixed.u_axis, Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(fixed.v_scale, MAX_SCALE);
    assert_eq!(fixed.v_shift, MAX_SHIFT);
    // Parallel axes are degenerate too.
    let parallel = TexInfo {
        v_axis: fixed.u_axis,
        ..fixed
    };
    assert!(!parallel.is_valid());
    assert!(parallel.sanitized(n).is_valid());
}

#[test]
fn operations_ignore_non_finite_arguments_and_clamp() {
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    let before = t;
    t.rotate(f32::NAN);
    t.shift_by(f32::NAN, 1.0);
    t.scale_by(f32::INFINITY, 2.0);
    t.scale_by(0.0, 2.0);
    t.follow_translation(p(f64::NAN, 0.0, 0.0));
    assert_eq!(t, before);
    t.scale_by(1.0e9, 1.0e-9);
    assert_eq!((t.u_scale, t.v_scale), (MAX_SCALE, MIN_SCALE));
    t.shift_by(1.0e9, -1.0e9);
    assert_eq!((t.u_shift, t.v_shift), (MAX_SHIFT, -MAX_SHIFT));
    let nan_scale = TexInfo {
        u_scale: f32::NAN,
        ..t
    };
    assert_eq!(nan_scale.uv_at(p(1.0, 1.0, 1.0), TEX), (0.0, 0.0));
    assert!(t.uv_at(p(1.0, 1.0, 0.0), (0, 0)).0.is_finite());
}

#[test]
fn fit_spans_the_requested_tiles() {
    let face = floor(2.0, 3.0, 12.0, 8.0); // 10 x 5
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    assert!(t.fit(&face, TEX, 1.0, 2.0));
    close(t.uv_at(p(2.0, 3.0, 0.0), TEX), (0.0, 0.0));
    close(t.uv_at(p(12.0, 8.0, 0.0), TEX), (1.0, 2.0));
    assert!(!t.fit(&face, TEX, 0.0, 1.0));
    assert!(!t.fit(&face, TEX, f32::NAN, 1.0));
    let line = poly(&[p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0), p(10.0, 0.0, 0.0)]);
    let mut u = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    assert!(!u.fit(&line, TEX, 1.0, 1.0), "no extent along v");
    // A flipped texture stays flipped but still spans the tiles.
    let mut f = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    f.flip_u();
    assert!(f.fit(&face, TEX, 1.0, 1.0));
    close(f.uv_at(p(2.0, 3.0, 0.0), TEX), (1.0, 0.0));
    close(f.uv_at(p(12.0, 3.0, 0.0), TEX), (0.0, 0.0));
}

#[test]
fn justify_touches_the_chosen_edge() {
    let face = floor(2.0, 3.0, 4.0, 7.0);
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    assert!(t.justify(&face, TEX, Justify::Left));
    assert!(off_integer(t.uv_at(p(2.0, 3.0, 0.0), TEX).0) < 1e-4);
    assert!(t.justify(&face, TEX, Justify::Right));
    assert!(off_integer(t.uv_at(p(4.0, 3.0, 0.0), TEX).0) < 1e-4);
    assert!(t.justify(&face, TEX, Justify::Top));
    assert!(off_integer(t.uv_at(p(2.0, 3.0, 0.0), TEX).1) < 1e-4);
    assert!(t.justify(&face, TEX, Justify::Bottom));
    assert!(off_integer(t.uv_at(p(2.0, 7.0, 0.0), TEX).1) < 1e-4);
    assert!(t.justify(&face, TEX, Justify::Center));
    let mid = t.uv_at(p(3.0, 5.0, 0.0), TEX);
    close((mid.0.rem_euclid(1.0), mid.1.rem_euclid(1.0)), (0.5, 0.5));
}

#[test]
fn align_to_world_resets_axes_but_keeps_scale() {
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    t.set_scale(0.5, 0.5);
    t.rotate(33.0);
    t.align_to_world(p(0.0, 1.0, 0.0));
    assert_eq!(t.u_axis, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(t.v_axis, Vec3::new(0.0, 0.0, 1.0));
    assert_eq!((t.rotation_deg, t.u_scale), (0.0, 0.5));
}

#[test]
fn align_to_face_follows_the_long_horizontal_edge() {
    // A tall narrow wall facing +x: the vertical edge is longest.
    let wall = poly(&[
        p(0.0, 0.0, 0.0),
        p(0.0, 2.0, 0.0),
        p(0.0, 2.0, 9.0),
        p(0.0, 0.0, 9.0),
    ]);
    let mut t = TexInfo::default_for_normal(p(1.0, 0.0, 0.0));
    t.rotate(40.0);
    assert!(t.align_to_face(&wall, p(1.0, 0.0, 0.0)));
    assert_eq!(t.u_axis, Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(t.v_axis, Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(t.rotation_deg, 0.0);
    // A tilted floor quad: u follows its long edge, v is perpendicular.
    let slab = poly(&[
        p(0.0, 0.0, 0.0),
        p(6.0, 6.0, 0.0),
        p(5.0, 7.0, 0.0),
        p(-1.0, 1.0, 0.0),
    ]);
    assert!(t.align_to_face(&slab, p(0.0, 0.0, 1.0)));
    let (u, v) = (t.u_axis.to_f64(), t.v_axis.to_f64());
    assert!(u.dot(v).abs() < 1e-6 && u.x > 0.0 && u.y > 0.0);
    assert!(!t.align_to_face(&slab, p(0.0, 0.0, 0.0)));
}

#[test]
fn follow_translation_moves_unlocked_textures_with_the_brush() {
    let at0 = p(1.0, 2.0, 0.0);
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    let before = t.uv_at(at0, TEX);
    t.follow_translation(p(4.0, 0.0, 0.0));
    close(t.uv_at(at0, TEX), before); // locked: the world texture is unchanged
    t.lock_to_world = false;
    t.follow_translation(p(4.0, -3.0, 9.0));
    close(t.uv_at(p(5.0, -1.0, 9.0), TEX), before);
}

/// The wall at x = 0 and the floor at z = 0 meet along the y axis.
fn floor_and_wall() -> (Polygon, Polygon) {
    let floor = poly(&[
        p(0.0, 0.0, 0.0),
        p(8.0, 0.0, 0.0),
        p(8.0, 5.0, 0.0),
        p(0.0, 5.0, 0.0),
    ]);
    let wall = poly(&[
        p(0.0, 0.0, 0.0),
        p(0.0, 5.0, 0.0),
        p(0.0, 5.0, 6.0),
        p(0.0, 0.0, 6.0),
    ]);
    (floor, wall)
}

#[test]
fn copy_across_edge_continues_the_texture_around_a_corner() {
    let (floor, wall) = floor_and_wall();
    let mut src = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    src.rotate(25.0);
    src.set_scale(0.1, 0.2);
    src.shift_by(7.0, 11.0);
    let dst = src.copy_across_edge(&floor, &wall).expect("shared edge");
    assert_eq!(dst.rotation_deg, 0.0);
    assert_eq!(dst.material, src.material);
    // Points on the shared edge map identically.
    for y in [0.0, 2.5, 5.0] {
        close(
            dst.uv_at(p(0.0, y, 0.0), TEX),
            src.uv_at(p(0.0, y, 0.0), TEX),
        );
    }
    // Climbing the wall by d continues like walking d onto the floor
    // (the wall unfolds flat into the floor plane, towards -x).
    for (y, d) in [(1.0, 2.0), (4.0, 5.5)] {
        close(dst.uv_at(p(0.0, y, d), TEX), src.uv_at(p(-d, y, 0.0), TEX));
    }
    assert!(dst.is_valid());
}

#[test]
fn copy_across_edge_handles_coplanar_neighbours_and_rejects_strangers() {
    let a = floor(0.0, 0.0, 4.0, 4.0);
    let b = floor(4.0, 0.0, 9.0, 4.0);
    let mut t = TexInfo::default_for_normal(p(0.0, 0.0, 1.0));
    t.rotate(-17.0);
    t.shift_by(3.0, 5.0);
    let copied = t.copy_across_edge(&a, &b).expect("shared edge");
    for q in [p(5.0, 1.0, 0.0), p(8.0, 3.0, 0.0), p(4.0, 2.0, 0.0)] {
        close(copied.uv_at(q, TEX), t.uv_at(q, TEX));
    }
    let far = floor(20.0, 20.0, 24.0, 24.0);
    assert!(t.copy_across_edge(&a, &far).is_none());
}

#[test]
fn copy_across_a_ceiling_edge() {
    // Wall x = 0 (facing +x) against the ceiling at z = 6 (facing down).
    let wall = poly(&[
        p(0.0, 0.0, 0.0),
        p(0.0, 5.0, 0.0),
        p(0.0, 5.0, 6.0),
        p(0.0, 0.0, 6.0),
    ]);
    let ceil = poly(&[
        p(0.0, 5.0, 6.0),
        p(8.0, 5.0, 6.0),
        p(8.0, 0.0, 6.0),
        p(0.0, 0.0, 6.0),
    ]);
    let mut t = TexInfo::default_for_normal(p(1.0, 0.0, 0.0));
    t.shift_by(9.0, 4.0);
    let c = t.copy_across_edge(&wall, &ceil).expect("shared edge");
    close(
        c.uv_at(p(0.0, 3.0, 6.0), TEX),
        t.uv_at(p(0.0, 3.0, 6.0), TEX),
    );
    // Walking d along the ceiling from the edge continues up the wall.
    close(
        c.uv_at(p(3.0, 2.0, 6.0), TEX),
        t.uv_at(p(0.0, 2.0, 9.0), TEX),
    );
}
