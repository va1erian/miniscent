//! The shipped textures must not carry aliased diagonals: magnified bilinear
//! sampling turns a hard one-texel staircase into a visible zigzag along the
//! hazard stripes of the floor (seen as jagged shadow edges on striped floor).

use std::path::Path;

use mscent_render::load_texture;

#[test]
fn hazard_stripes_have_antialiased_diagonal_edges() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let t = load_texture(&dir, "floor").expect("floor texture");
    // Inside the stripe band (rows 48..80), away from the grout row at 64.
    let (mut total, mut mid) = (0, 0);
    for y in 50..62 {
        for x in 4..124 {
            let red = (t.texel(x, y) >> 16) & 0xff;
            total += 1;
            // Dark stripe is about 20, hazard yellow at least 150.
            mid += i32::from((60..140).contains(&red));
        }
    }
    let pct = 100.0 * mid as f32 / total as f32;
    assert!(pct > 3.0, "only {pct:.1}% of stripe pixels are blended");
}

/// The game makes every listed recipe at startup, on one thread. The shipped stock,
/// the four 512 px sky recipes included, must sit far inside the limits a directory
/// is held to, so that the limits stay a guard against hostile files and not a
/// ceiling on the stock.
#[test]
fn the_shipped_stock_is_far_inside_the_directory_limits() {
    use mscent_render::manifest::{self, Entry};
    use mscent_render::recipe_texture::{
        MAX_DIRECTORY_NS, MAX_DIRECTORY_TEXELS, check_directory_work,
    };
    use mscent_texgen::pack::bake_cost;
    use mscent_texgen::recipe;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let entries = manifest::parse(&std::fs::read(dir.join("materials.txt")).expect("manifest"))
        .expect("parses");
    check_directory_work(&dir, &entries).expect("the directory passes");
    let (mut ns, mut texels) = (0u64, 0u64);
    for entry in &entries {
        let Entry::Material { name, .. } = entry else {
            continue;
        };
        let bytes = std::fs::read(dir.join(format!("{name}.mst"))).expect("recipe");
        let r = recipe::read(&bytes).expect("reads");
        ns += bake_cost(&r, r.size).expect("within its own limit");
        texels += r.size.texels() as u64;
    }
    eprintln!("stock: {ns} ns of {MAX_DIRECTORY_NS}, {texels} texels of {MAX_DIRECTORY_TEXELS}");
    assert!(ns * 10 < MAX_DIRECTORY_NS, "{ns} ns is within a tenth");
    assert!(texels * 10 < MAX_DIRECTORY_TEXELS, "{texels} texels");
}
