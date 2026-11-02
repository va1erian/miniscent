//! Review check: whatever sequence of editor operations (including hostile
//! arguments) is applied to a TexInfo, the result must stay valid, survive a
//! save/load round trip unchanged, and keep producing finite coordinates.

use mscent_math::rng::Rng;
use mscent_math::{Polygon, Vec3d};
use mscent_world::texinfo::Justify;
use mscent_world::{Brush, Level, Op, TexInfo, format};

fn special(rng: &mut Rng) -> f32 {
    match rng.below(9) {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => 0.0,
        4 => -0.0,
        5 => f32::MAX,
        6 => f32::MIN_POSITIVE / 2.0,
        7 => -1.0e-3,
        _ => (rng.next_u64() % 20_000) as f32 / 100.0 - 100.0,
    }
}

#[test]
fn operation_sequences_keep_texinfo_valid_and_round_trippable() {
    let wall = Polygon::new(vec![
        Vec3d::new(0.0, 0.0, 0.0),
        Vec3d::new(0.0, 40.0, 0.0),
        Vec3d::new(0.0, 40.0, 30.0),
        Vec3d::new(0.0, 0.0, 30.0),
    ])
    .expect("wall");
    let n = Vec3d::new(1.0, 0.0, 0.0);
    let mut rng = Rng::new(3);
    for _ in 0..300 {
        let mut t = TexInfo::default_for_normal(n);
        t.lock_to_world = rng.below(2) == 0;
        for _ in 0..12 {
            let (a, b) = (special(&mut rng), special(&mut rng));
            match rng.below(11) {
                0 => t.rotate(a),
                1 => t.shift_by(a, b),
                2 => t.scale_by(a, b),
                3 => t.set_scale(a, b),
                4 => t.flip_u(),
                5 => t.flip_v(),
                6 => {
                    t.fit(&wall, (128, 64), a, b);
                }
                7 => {
                    let how = [Justify::Left, Justify::Right, Justify::Top, Justify::Bottom];
                    t.justify(&wall, (128, 64), how[rng.below(4) as usize]);
                }
                8 => {
                    t.align_to_face(&wall, n);
                }
                9 => t.align_to_world(n),
                _ => t.follow_translation(Vec3d::new(f64::from(a), f64::from(b), 3.0)),
            }
            assert!(t.is_valid(), "invalid after op: {t:?}");
            let (u, v) = t.uv_at(Vec3d::new(1.0e4, -2.0e4, 5.0e3), (128, 64));
            assert!(u.is_finite() && v.is_finite());
        }
        let mut brush = Brush::cuboid(
            Vec3d::new(0.0, 0.0, 0.0),
            Vec3d::new(8.0, 8.0, 8.0),
            Op::Add,
            0,
        )
        .expect("brush");
        let i = brush.plane_facing(n).expect("plane");
        assert!(brush.set_texinfo(i, t));
        let level = Level {
            brushes: vec![brush],
            entities: vec![],
            ..Default::default()
        };
        let back = format::read(&format::write(&level)).expect("round trip");
        assert_eq!(back, level);
    }
}
