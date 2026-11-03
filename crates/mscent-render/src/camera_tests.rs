use super::*;

fn cam() -> Camera {
    Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        core::f32::consts::FRAC_PI_2,
        0.1,
        100.0,
        64,
        48,
    )
    .unwrap()
}

#[test]
fn forward_point_projects_to_screen_centre() {
    let p = cam().project(Vec3::new(0.0, 0.0, -4.0)).unwrap();
    assert_eq!((p.x, p.y, p.inv_z), (32.0, 24.0, 0.25));
}

#[test]
fn right_and_up_map_to_screen_right_and_up() {
    let c = cam();
    // 90 degree vertical fov: at depth 2 the top edge is at y = 2.
    let top = c.project(Vec3::new(0.0, 2.0, -2.0)).unwrap();
    assert!((top.y - 0.0).abs() < 1e-4);
    let right = c.project(Vec3::new(1.0, 0.0, -2.0)).unwrap();
    assert!(right.x > 32.0);
}

#[test]
fn matches_the_matrix_projection() {
    let c = cam();
    let m = c.projection_matrix();
    for p in [Vec3::new(1.0, 0.5, -3.0), Vec3::new(-2.0, -1.0, -5.0)] {
        let h = m.transform_point(p);
        let sx = (h.x / h.w * 0.5 + 0.5) * 64.0;
        let sy = (0.5 - h.y / h.w * 0.5) * 48.0;
        let s = c.project(p).unwrap();
        assert!((s.x - sx).abs() < 1e-3 && (s.y - sy).abs() < 1e-3);
    }
}

#[test]
fn behind_the_eye_does_not_project() {
    assert!(cam().project(Vec3::new(0.0, 0.0, 1.0)).is_none());
    assert!(cam().project(Vec3::new(0.0, 0.0, 0.0)).is_none());
}

#[test]
fn world_to_camera_moves_with_the_eye() {
    let c = Camera::look_at(
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(10.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        1.0,
        0.1,
        50.0,
        16,
        16,
    )
    .unwrap();
    let p = c.world_to_camera(Vec3::new(10.0, 0.0, -5.0));
    assert!((p.z + 5.0).abs() < 1e-5 && p.x.abs() < 1e-5);
}

#[test]
fn frustum_membership() {
    let f = cam().frustum();
    assert!(f.contains(Vec3::new(0.0, 0.0, -5.0)));
    assert!(!f.contains(Vec3::new(0.0, 0.0, -0.05)), "nearer than near");
    assert!(!f.contains(Vec3::new(0.0, 0.0, -101.0)), "beyond far");
    assert!(!f.contains(Vec3::new(20.0, 0.0, -5.0)), "right of view");
    assert!(!f.contains(Vec3::new(0.0, -20.0, -5.0)), "below view");
}

#[test]
fn rejects_invalid_parameters() {
    let eye = Vec3::new(0.0, 0.0, 0.0);
    let t = Vec3::new(0.0, 0.0, -1.0);
    let up = Vec3::new(0.0, 1.0, 0.0);
    assert!(Camera::look_at(eye, t, up, 1.0, 0.1, 50.0, 0, 10).is_none());
    assert!(Camera::look_at(eye, t, up, 1.0, 0.1, 50.0, 10, 9000).is_none());
    assert!(Camera::look_at(eye, t, up, 0.0, 0.1, 50.0, 10, 10).is_none());
    assert!(Camera::look_at(eye, t, up, 1.0, 0.0, 50.0, 10, 10).is_none());
    assert!(Camera::look_at(eye, eye, up, 1.0, 0.1, 50.0, 10, 10).is_none());
}

#[test]
fn the_gpu_matrix_lands_where_project_does() {
    let cam = Camera::look_at(
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(4.0, 2.0, 3.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.3,
        0.1,
        400.0,
        320,
        180,
    )
    .expect("a camera");
    let matrix = cam.gpu_clip_matrix();
    for point in [
        Vec3::new(5.0, 2.0, 3.0),
        Vec3::new(9.0, 4.0, 1.0),
        Vec3::new(300.0, -20.0, 40.0),
    ] {
        let want = cam.project(cam.world_to_camera(point)).expect("in front");
        let clip = matrix.transform_point(point);
        let (x, y, depth) = (clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
        assert!((160.0 + x * 160.0 - want.x).abs() < 1e-2);
        assert!((90.0 - y * 90.0 - want.y).abs() < 1e-2);
        assert!((0.0..=1.0).contains(&depth));
        assert!((cam.inv_z_of_gpu_depth(depth) - want.inv_z).abs() < want.inv_z * 1e-4);
    }
    assert_eq!(cam.inv_z_of_gpu_depth(0.0), 0.0);
}

#[test]
fn gpu_depth_meets_one_at_near_and_zero_at_far() {
    let cam = Camera::look_at(
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.3,
        0.1,
        400.0,
        320,
        180,
    )
    .expect("a camera");
    let depth_at = |distance: f32| {
        let clip = cam
            .gpu_clip_matrix()
            .transform_point(Vec3::new(distance, 0.0, 0.0));
        clip.z / clip.w
    };
    assert!((depth_at(0.1) - 1.0).abs() < 1e-3);
    assert!(depth_at(400.0).abs() < 1e-3);
}
