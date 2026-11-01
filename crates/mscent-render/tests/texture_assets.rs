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
