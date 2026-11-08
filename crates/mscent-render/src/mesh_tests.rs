use super::*;

const N: u32 = 32;
const BG: u32 = 0xff00_0000;

fn camera(yaw: f32, pitch: f32, dist: f32) -> Camera {
    let dir = Vec3::new(
        yaw.cos() * pitch.cos(),
        yaw.sin() * pitch.cos(),
        pitch.sin(),
    );
    let eye = dir * dist;
    Camera::look_at(
        eye,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, 1.0),
        0.8,
        0.1,
        100.0,
        N,
        N,
    )
    .unwrap()
}

fn pose() -> Pose {
    Pose {
        pos: Vec3::ZERO,
        yaw: 0.0,
        scale: 1.0,
    }
}

fn render(mesh: &Mesh, cam: &Camera, lighting: &MeshLighting) -> Framebuffer {
    let mut fb = Framebuffer::new(N, N).unwrap();
    fb.clear(BG);
    draw_mesh(&mut fb, cam, mesh, &pose(), lighting, None);
    fb
}

fn silhouette(fb: &Framebuffer) -> Vec<bool> {
    fb.pixels().iter().map(|&p| p != BG).collect()
}

fn covered(fb: &Framebuffer) -> usize {
    silhouette(fb).iter().filter(|&&b| b).count()
}

#[test]
fn constructors_validate() {
    assert!(Mesh::bipyramid(2, 1.0, 1.0, 1.0, 0).is_none());
    assert!(Mesh::bipyramid(33, 1.0, 1.0, 1.0, 0).is_none());
    assert!(Mesh::bipyramid(4, 0.0, 1.0, 1.0, 0).is_none());
    assert!(Mesh::bipyramid(4, 1.0, f32::NAN, 1.0, 0).is_none());
    let v = MeshVertex {
        pos: Vec3::ZERO,
        normal: Vec3::new(0.0, 0.0, 1.0),
    };
    let bad = Triangle {
        indices: [0, 1, 2],
        color: 0,
    };
    assert!(Mesh::new(vec![v, v], vec![bad]).is_none(), "index 2 of 2");
    assert!(Mesh::new(vec![v, v, v], vec![bad]).is_some());
    assert_eq!(Mesh::goal().triangles().len(), 56);
}

#[test]
fn every_angle_shows_the_mesh_and_the_silhouette_varies() {
    let lighting = MeshLighting::default();
    let mesh = Mesh::goal();
    let mut shapes = Vec::new();
    for (yaw, pitch) in [(0.0, 0.0), (1.2, 0.0), (2.5, 0.3), (4.0, -0.3), (0.0, 1.2)] {
        let fb = render(&mesh, &camera(yaw, pitch, 3.2), &lighting);
        let n = covered(&fb);
        assert!(n > 25 && n < 300, "yaw {yaw} pitch {pitch}: {n} pixels");
        shapes.push(silhouette(&fb));
    }
    assert_ne!(shapes[0], shapes[4], "side and top views must differ");
}

#[test]
fn goal_silhouette_repeats_every_half_turn() {
    let goal = Mesh::goal();
    let lighting = MeshLighting::default();
    let a = silhouette(&render(&goal, &camera(0.5, 0.2, 3.2), &lighting));
    let b = silhouette(&render(
        &goal,
        &camera(0.5 + core::f32::consts::PI, 0.2, 3.2),
        &lighting,
    ));
    let differing = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    assert!(differing <= 2, "{differing} pixels differ");
}

#[test]
fn light_comes_from_the_sun_side() {
    let goal = Mesh::goal();
    let lit = MeshLighting {
        ambient: 0.1,
        sun_dir: Vec3::new(1.0, 0.0, 0.0),
        sun: 1.0,
        points: Vec::new(),
    };
    let cam = camera(core::f32::consts::FRAC_PI_2, 0.0, 3.2); // looking along -Y
    let fb = render(&goal, &cam, &lit);
    // Camera at +Y looking at the origin: image right is world -X, so the
    // sun side is the left half of the covered area.
    let xs: Vec<i32> = (0..N as i32)
        .filter(|&x| (0..N as i32).any(|y| fb.pixel(x, y) != Some(BG)))
        .collect();
    let mid = (xs[0] + xs[xs.len() - 1]) / 2;
    let red_sum = |range: core::ops::Range<i32>| -> u64 {
        range
            .flat_map(|x| (0..N as i32).map(move |y| (x, y)))
            .filter_map(|(x, y)| fb.pixel(x, y))
            .filter(|&p| p != BG)
            .map(|p| u64::from((p >> 16) & 255))
            .sum()
    };
    let (toward_sun, away) = (red_sum(xs[0]..mid), red_sum(mid + 1..xs[xs.len() - 1] + 1));
    assert!(toward_sun > away, "{toward_sun} vs {away}");
}

#[test]
fn point_light_brightens_nearby_surfaces() {
    let mesh = Mesh::goal();
    let dark = MeshLighting {
        ambient: 0.2,
        sun: 0.0,
        ..MeshLighting::default()
    };
    let mut near = dark.clone();
    near.points.push(PointLight {
        pos: Vec3::new(0.0, 3.0, 2.0),
        radius: 10.0,
        intensity: 1.5,
    });
    let cam = camera(core::f32::consts::FRAC_PI_2, 0.0, 3.2);
    let sum =
        |fb: &Framebuffer| -> u64 { fb.pixels().iter().map(|&p| u64::from((p >> 8) & 255)).sum() };
    assert!(sum(&render(&mesh, &cam, &near)) > sum(&render(&mesh, &cam, &dark)));
}

#[test]
fn walls_hide_meshes_through_the_shared_depth_buffer() {
    let cam = camera(0.0, 0.0, 3.2);
    let mut fb = Framebuffer::new(N, N).unwrap();
    fb.clear(BG);
    let wall = [
        Vertex::new(Vec3::new(1.5, -9.0, 9.0), 0.0, 0.0),
        Vertex::new(Vec3::new(1.5, 9.0, 9.0), 1.0, 0.0),
        Vertex::new(Vec3::new(1.5, 9.0, -9.0), 1.0, 1.0),
        Vertex::new(Vec3::new(1.5, -9.0, -9.0), 0.0, 1.0),
    ];
    crate::draw_polygon_flat(
        &mut fb,
        &cam,
        &wall,
        &Texture::solid(0xff11_2233),
        256,
        None,
    );
    let before = fb.pixels().to_vec();
    let st = draw_mesh(
        &mut fb,
        &cam,
        &Mesh::goal(),
        &pose(),
        &MeshLighting::default(),
        None,
    );
    assert_eq!(st.pixels, 0);
    assert_eq!(fb.pixels(), &before[..], "mesh is behind the wall");
}

#[test]
fn fog_pulls_colour_toward_the_fog_colour() {
    let cam = camera(0.0, 0.0, 3.2);
    let fog = Fog::new(0xffff_ffff, 0.0, 6.0);
    let mut fb = Framebuffer::new(N, N).unwrap();
    fb.clear(BG);
    draw_mesh(
        &mut fb,
        &cam,
        &Mesh::goal(),
        &pose(),
        &MeshLighting::default(),
        fog,
    );
    let plain = render(&Mesh::goal(), &cam, &MeshLighting::default());
    let blue = |fb: &Framebuffer| -> u64 { fb.pixels().iter().map(|&p| u64::from(p & 255)).sum() };
    assert!(blue(&fb) > blue(&plain));
}

#[test]
fn hostile_poses_and_cameras_draw_nothing_without_panicking() {
    let cam = camera(0.0, 0.0, 3.2);
    let mut fb = Framebuffer::new(N, N).unwrap();
    for bad in [f32::NAN, f32::INFINITY] {
        let p = Pose {
            scale: bad,
            ..pose()
        };
        let st = draw_mesh(
            &mut fb,
            &cam,
            &Mesh::goal(),
            &p,
            &MeshLighting::default(),
            None,
        );
        assert_eq!(st, DrawStats::default());
    }
    // Camera inside the mesh and far behind it: just must not panic.
    let inside = Camera::look_at(
        Vec3::new(0.0, 0.0, 0.1),
        Vec3::new(1.0, 0.0, 0.1),
        Vec3::new(0.0, 0.0, 1.0),
        1.0,
        0.05,
        50.0,
        N,
        N,
    )
    .unwrap();
    draw_mesh(
        &mut fb,
        &inside,
        &Mesh::goal(),
        &pose(),
        &MeshLighting::default(),
        None,
    );
}

#[test]
fn sphere_normals_are_unit_and_validated() {
    let m = Mesh::sphere(6, 10, 2.0, 0xffff_ffff).unwrap();
    for v in m.vertices() {
        assert!((v.normal.length() - 1.0).abs() < 1e-4);
        assert!((v.pos.length() - 2.0).abs() < 1e-4);
    }
    // Two fans of 10 plus four bands of 10 quads.
    assert_eq!(m.triangles().len(), 20 + 4 * 20);
    assert!(Mesh::sphere(1, 10, 1.0, 0).is_none());
    assert!(Mesh::sphere(6, 2, 1.0, 0).is_none());
    assert!(Mesh::sphere(6, 10, f32::NAN, 0).is_none());
}
