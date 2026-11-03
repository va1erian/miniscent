//! The player's ship reads and builds, and stays inside the model limits.

use mscent_world::model::{build, read};

#[test]
fn the_player_ship_reads_and_builds() {
    let text = include_str!("../../../assets/ship/player.msb");
    let blueprint = read(text.as_bytes()).expect("the ship reads");
    let model = build(&blueprint).expect("the ship builds");
    assert_eq!(blueprint.parts.len(), 1);
    let tris: usize = model.parts.iter().map(|p| p.lods[0].triangles.len()).sum();
    println!("the player ship: {} bytes, {tris} triangles", text.len());
    assert!(tris > 100 && tris < 4000, "{tris}");
}
