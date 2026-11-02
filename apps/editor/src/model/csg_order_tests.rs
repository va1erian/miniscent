use mscent_world::Op;

use super::document::{BrushDef, Document};

#[test]
fn volumes_survive_loading_into_a_document_and_saving_again() {
    use mscent_world::voxel::{CellSize, Volume};
    let mut level = mscent_world::showcase_level();
    let last = level
        .brushes
        .iter()
        .map(|b| b.order())
        .max()
        .expect("brushes");
    let volume = Volume::new(Op::Add, last + 5, CellSize::Quarter, [0; 3], [1, 1, 1]);
    level.volumes.push(volume.expect("valid"));
    let doc = Document::from_level(&level);
    let saved = doc.to_level().expect("exports");
    let renumbered = level.volumes[0]
        .clone()
        .with_order(level.brushes.len() as u32);
    assert_eq!(saved.volumes, [renumbered]);
    let built = crate::build::validate(&doc).expect("valid");
    assert_eq!(built.volumes, saved.volumes);
}

/// The volume's place among the brushes, as the orders of the brushes around it.
fn sides_of_volume(doc: &Document) -> (Vec<usize>, Vec<usize>) {
    let level = doc.to_level().expect("exports");
    let at = level.volumes[0].order();
    let ids = |keep: &dyn Fn(u32) -> bool| {
        (level.brushes.iter().enumerate())
            .filter(|(_, b)| keep(b.order()))
            .map(|(i, _)| doc.brushes()[i].id as usize)
            .collect()
    };
    (ids(&|o| o < at), ids(&|o| o > at))
}

#[test]
fn a_volume_keeps_its_place_among_brushes_across_saving_and_edits() {
    use mscent_math::Vec3d;
    use mscent_world::Level;
    use mscent_world::voxel::{CellSize, Volume};
    let cube = |x: f64, order: u32| {
        let lo = Vec3d::new(x, 0.0, 0.0);
        mscent_world::Brush::cuboid(lo, Vec3d::new(x + 1.0, 1.0, 1.0), Op::Add, order)
            .expect("valid")
    };
    let level = Level {
        brushes: vec![cube(0.0, 10), cube(2.0, 20), cube(4.0, 30)],
        volumes: vec![Volume::new(Op::Add, 25, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("ok")],
        ..Default::default()
    };
    let mut doc = Document::from_level(&level);
    let ids: Vec<usize> = doc.brushes().iter().map(|b| b.id as usize).collect();
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    assert_eq!(sides_of_volume(&doc), (vec![a, b], vec![c]));

    // Saving and loading again keeps it, and every order stays distinct.
    let again = Document::from_level(&doc.to_level().expect("exports"));
    assert_eq!(sides_of_volume(&again).0.len(), 2);

    // Deleting the brush right before it leaves it after the one before that,
    // and undoing the delete restores the place exactly.
    let (at, removed) = doc.remove_brush(b as u32).expect("there");
    assert_eq!(sides_of_volume(&doc), (vec![a], vec![c]));
    doc.insert_brush(at, removed);
    assert_eq!(sides_of_volume(&doc), (vec![a, b], vec![c]));

    // A brush added at the end comes after the volume and cuts it.
    let added = doc.alloc_id();
    let def = BrushDef::from_brush(added, &cube(6.0, 0));
    doc.insert_brush(3, def);
    assert_eq!(sides_of_volume(&doc), (vec![a, b], vec![c, added as usize]));
}
