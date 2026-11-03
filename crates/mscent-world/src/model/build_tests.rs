use std::collections::HashMap;

use mscent_math::Vec3;

use super::*;

const SENTRY: &str = include_str!("../../../../assets/enemies/sentry.msb");
const DART: &str = include_str!("../../../../assets/enemies/dart.msb");
const MANTIS: &str = include_str!("../../../../assets/enemies/mantis.msb");
const HEAD: &str = "miniscent-blueprint 1\nname a\nclass drone\nskin hull metal\nskin trim floor\n";

fn model(body: &str) -> Model {
    build(&read(format!("{HEAD}{body}").as_bytes()).unwrap()).unwrap()
}

fn stock(text: &str) -> Model {
    build(&read(text.as_bytes()).unwrap()).unwrap()
}

/// A position as a key. Adding zero makes -0 and 0 the same.
fn bits(p: Vec3) -> [u32; 3] {
    [p.x + 0.0, p.y + 0.0, p.z + 0.0].map(f32::to_bits)
}

/// How many edges, by position, are not matched by one going the other way: zero
/// is a closed surface.
fn open_edges(mesh: &PartMesh) -> usize {
    let mut edges: HashMap<([u32; 3], [u32; 3]), i32> = HashMap::new();
    for t in &mesh.triangles {
        for i in 0..3 {
            let a = bits(mesh.vertices[usize::from(t[i])].pos);
            let b = bits(mesh.vertices[usize::from(t[(i + 1) % 3])].pos);
            // Two cells can put their vertices in one place. That edge has no length.
            if a == b {
                continue;
            }
            if a < b {
                *edges.entry((a, b)).or_default() += 1;
            } else {
                *edges.entry((b, a)).or_default() -= 1;
            }
        }
    }
    edges.values().filter(|&&n| n != 0).count()
}

/// How many triangles face away from what their vertices say, which a renderer
/// would cull wrongly.
fn facing_inward(mesh: &PartMesh) -> usize {
    let inward = |t: &&[u16; 3]| {
        let [a, b, c] = t.map(|i| mesh.vertices[usize::from(i)]);
        let n = (b.pos - a.pos).cross(c.pos - a.pos);
        n.length() > 1e-3 && n.dot(a.normal + b.normal + c.normal) <= 0.0
    };
    mesh.triangles.iter().filter(inward).count()
}

fn triangles(m: &Model, level: usize) -> usize {
    m.parts.iter().map(|p| p.lods[level].triangles.len()).sum()
}

#[test]
fn the_same_bytes_give_the_same_mesh() {
    for text in [SENTRY, DART, MANTIS] {
        assert_eq!(stock(text), stock(text));
    }
}

#[test]
fn every_stock_model_faces_outward_with_roles_in_range() {
    for text in [SENTRY, DART, MANTIS] {
        let b = read(text.as_bytes()).unwrap();
        let m = build(&b).unwrap();
        assert!(triangles(&m, 0) > 0);
        for part in &m.parts {
            for mesh in &part.lods {
                // A voxel normal is read over neighbouring cells, so a thin spot can
                // disagree with its triangle. That must stay rare at every level.
                let wrong = facing_inward(mesh);
                assert!(wrong * 50 <= mesh.triangles.len(), "{wrong} face inward");
                assert_eq!(mesh.triangles.len(), mesh.roles.len());
                assert!(mesh.roles.iter().all(|&r| usize::from(r) < b.skins.len()));
                for v in &mesh.vertices {
                    assert!((v.normal.length() - 1.0).abs() < 1e-3);
                    let inside = mesh.bounds.unwrap();
                    assert!(v.pos.x >= inside.min.x && v.pos.x <= inside.max.x);
                }
            }
        }
    }
}

#[test]
fn a_solid_is_a_closed_surface_with_hard_edges() {
    let m = model("part p\nsolid cube 0 0 0 2 2 2 hull\n");
    let mesh = &m.parts[0].lods[0];
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(
        mesh.vertices.len(),
        24,
        "a corner has a vertex for each face"
    );
    assert_eq!(open_edges(mesh), 0);
    let b = mesh.bounds.unwrap();
    assert_eq!(b.min, Vec3::new(-2.0, -2.0, -2.0));
    assert_eq!(b.max, Vec3::new(2.0, 2.0, 2.0));
}

#[test]
fn a_voxel_part_is_closed_across_chunks() {
    // Wide enough to cross chunk borders at this cell.
    let m = model("part p cell=0.25\nadd sphere 0.1 0.2 0.3 7 hull\n");
    let mesh = &m.parts[0].lods[0];
    assert!(mesh.triangles.len() > 1000);
    assert_eq!(open_edges(mesh), 0);
    let b = mesh.bounds.unwrap();
    assert!(
        (b.max.x - 7.1).abs() < 0.3 && (b.min.x + 6.9).abs() < 0.3,
        "{b:?}"
    );
}

#[test]
fn a_cut_opens_a_solid_and_lines_it_with_its_role() {
    let whole = model("part p\nsolid cube 0 0 0 4 4 4 hull\n");
    let cut = model("part p\nsolid cube 0 0 0 4 4 4 hull\ncut cube 4 0 0 2 2 2 trim\n");
    let mesh = &cut.parts[0].lods[0];
    assert_eq!(open_edges(mesh), 0, "the walls close the notch");
    assert_eq!(facing_inward(mesh), 0);
    assert!(mesh.triangles.len() > whole.parts[0].lods[0].triangles.len());
    assert!(mesh.roles.contains(&0) && mesh.roles.contains(&1));
    // Nothing of the brush around the CSG is left, and nothing past the solid.
    let b = mesh.bounds.unwrap();
    assert_eq!((b.min.x, b.max.x), (-4.0, 4.0));
    // The notch's floor is there.
    assert!(mesh.vertices.iter().any(|v| v.pos.x == 2.0));
}

#[test]
fn a_cut_also_cuts_the_voxels_of_its_part() {
    let whole = model("part p cell=0.25\nadd sphere 0 0 0 4 hull\n");
    let cut = model("part p cell=0.25\nadd sphere 0 0 0 4 hull\ncut cube 4 0 0 2 8 8\n");
    let reach = |m: &Model| m.parts[0].lods[0].bounds.unwrap().max.x;
    assert!(reach(&whole) > 3.8);
    assert!(reach(&cut) < 2.4, "{}", reach(&cut));
}

#[test]
fn a_cut_elsewhere_leaves_a_solid_direct() {
    let plain = model("part p\nsolid cube 0 0 0 2 2 2 hull\n");
    let far = model("part p\nsolid cube 0 0 0 2 2 2 hull\ncut cube 20 0 0 1 1 1\n");
    assert_eq!(plain.parts[0].lods[0], far.parts[0].lods[0]);
}

#[test]
fn only_solids_a_cut_touches_go_through_csg() {
    let m = model(
        "part p\nsolid cube 0 0 0 4 4 4 hull\ncut cube 4 0 0 2 2 2\nsolid cube 30 0 0 1 1 1 trim\n",
    );
    let mesh = &m.parts[0].lods[0];
    let far = mesh.vertices.iter().filter(|v| v.pos.x >= 29.0).count();
    assert_eq!(far, 24, "the far cube kept its faces as they were");
}

#[test]
fn detail_solids_go_below_full_detail() {
    let m = model("part p\nsolid cube 0 0 0 2 2 2 hull\nsolid cube 5 0 0 1 1 1 trim detail\n");
    let lods = &m.parts[0].lods;
    assert_eq!(lods[0].triangles.len(), 24);
    assert_eq!(lods[1].triangles.len(), 12);
    assert_eq!(lods[2].triangles.len(), 12);
}

#[test]
fn lower_levels_have_fewer_triangles() {
    let m = stock(MANTIS);
    assert!(triangles(&m, 1) < triangles(&m, 0));
    assert!(triangles(&m, 2) <= triangles(&m, 1));
    assert!(triangles(&m, 2) > 0);
}

#[test]
fn parts_keep_their_motion_and_stay_apart() {
    let b = read(SENTRY.as_bytes()).unwrap();
    let m = build(&b).unwrap();
    assert_eq!(m.parts.len(), 2);
    assert_eq!(m.parts[0].motion, Motion::Still);
    assert_eq!(m.parts[1].motion, Motion::Spin { rate: 0.25 });
    assert_eq!(m.parts[1].axis, Vec3::new(1.0, 0.0, 0.0));
}

#[test]
fn soft_and_hard_pieces_mix_and_keep_their_roles() {
    let m = model("part p cell=0.5\nsolid cube 0 0 0 3 1 1 hull\nadd sphere 3 0 0 2 trim\n");
    let mesh = &m.parts[0].lods[0];
    assert!(mesh.roles.contains(&0) && mesh.roles.contains(&1));
    assert_eq!(facing_inward(mesh), 0);
}

#[test]
fn painting_and_carving_change_the_voxels() {
    let plain = model("part p\nadd sphere 0 0 0 4 hull\n");
    let painted = model("part p\nadd sphere 0 0 0 4 hull\npaint sphere 4 0 0 2 trim\n");
    let carved = model("part p\nadd sphere 0 0 0 4 hull\ncarve sphere 4 0 0 2\n");
    assert!(!plain.parts[0].lods[0].roles.contains(&1));
    assert!(painted.parts[0].lods[0].roles.contains(&1));
    let reach = |m: &Model| m.parts[0].lods[0].bounds.unwrap().max.x;
    assert!(reach(&carved) <= reach(&plain));
    let tris = |m: &Model| m.parts[0].lods[0].triangles.len();
    assert_ne!(tris(&carved), tris(&plain));
}

#[test]
fn a_thin_limb_keeps_a_level_that_would_turn_it_inside_out() {
    // Radius 1.5 cells: merging cells here folds triangles over.
    let m = model("part p cell=0.5\nadd round_cone 0 0 0  12 3 2  0.75 0.5 hull\n");
    let lods = &m.parts[0].lods;
    for mesh in lods {
        assert!(facing_inward(mesh) * 50 <= mesh.triangles.len());
    }
    assert!(lods[2].triangles.len() <= lods[0].triangles.len());
}

#[test]
fn a_thick_body_still_loses_triangles_at_lower_levels() {
    let m = model("part p cell=0.25\nadd sphere 0 0 0 6 hull\n");
    let lods = &m.parts[0].lods;
    assert!(lods[1].triangles.len() * 2 < lods[0].triangles.len());
    assert!(lods[2].triangles.len() < lods[1].triangles.len());
}
