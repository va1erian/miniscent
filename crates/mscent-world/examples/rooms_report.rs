//! What the room table costs a level (M17.2): empty leaves, rooms, the table's
//! size and the time to build it, for every level in `assets/levels` and for
//! generated levels of 3, 5 and 8 rooms a side, and grids of 8, 14 and 18. Run from the repository root:
//!
//!   cargo run --release -p mscent-world --example rooms_report

use std::time::Instant;

use mscent_world::{
    GenParams, LeafKind, Level, Portals, Rooms, compile, extract_faces, format, generate_level,
};

struct Row {
    name: String,
    empty: usize,
    portals: usize,
    rooms: usize,
    bytes: usize,
    split_faces: usize,
    csg_ms: f64,
    rooms_ms: f64,
}

fn measure(name: String, level: &Level) -> Row {
    let brushes = level.csg_brushes();
    let t = Instant::now();
    let bsp = compile(&brushes).expect("a tree");
    let csg_ms = t.elapsed().as_secs_f64() * 1e3;
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    let t = Instant::now();
    let rooms = Rooms::build(&bsp, &portals);
    let rooms_ms = t.elapsed().as_secs_f64() * 1e3;
    let in_rooms = |f: &mscent_world::Face| {
        let mut r: Vec<u32> = f
            .front_leaves
            .iter()
            .filter_map(|&l| rooms.room_of(l))
            .collect();
        r.sort_unstable();
        r.dedup();
        r.len()
    };
    Row {
        name,
        empty: bsp
            .leaves()
            .iter()
            .filter(|l| l.kind == LeafKind::Empty)
            .count(),
        portals: portals.portals().len(),
        rooms: rooms.count(),
        bytes: rooms.table_bytes(),
        split_faces: faces.iter().filter(|f| in_rooms(f) > 1).count(),
        csg_ms,
        rooms_ms,
    }
}

/// A grid of `n` by `n` rooms 20 across with a 2 wide doorway between neighbours,
/// to see the table at sizes the generator does not make.
fn grid(n: u32) -> Level {
    let v = |x: f64, y: f64, z: f64| mscent_math::Vec3d::new(x, y, z);
    let mut brushes = Vec::new();
    let mut order = 0;
    let mut cut = |a: mscent_math::Vec3d, b: mscent_math::Vec3d| {
        order += 1;
        brushes.push(
            mscent_world::Brush::cuboid(a, b, mscent_world::Op::Subtract, order).expect("a brush"),
        );
    };
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (f64::from(i) * 24.0, f64::from(j) * 24.0);
            cut(v(x, y, 0.0), v(x + 20.0, y + 20.0, 8.0));
            if i + 1 < n {
                cut(v(x + 20.0, y + 9.0, 1.0), v(x + 24.0, y + 11.0, 3.0));
            }
            if j + 1 < n {
                cut(v(x + 9.0, y + 20.0, 1.0), v(x + 11.0, y + 24.0, 3.0));
            }
        }
    }
    Level {
        brushes,
        ..Level::default()
    }
}

fn main() {
    let mut levels: Vec<(String, Level)> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir("assets/levels")
        .expect("run from the repository root")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "msl"))
        .collect();
    files.sort();
    for f in files {
        let level = format::read(&std::fs::read(&f).expect("a file")).expect("a level");
        levels.push((
            f.file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned(),
            level,
        ));
    }
    for n in [3, 5, 8] {
        let params = GenParams {
            cols: n,
            rows: n,
            ..GenParams::default()
        };
        levels.push((format!("generated {n}x{n}"), generate_level(7, &params)));
    }
    for n in [8, 14, 18] {
        levels.push((format!("grid {n}x{n}"), grid(n)));
    }
    println!(
        "| level | empty leaves | portals | rooms | table bytes | faces in >1 room | csg ms | rooms ms |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for (name, level) in levels {
        let r = measure(name, &level);
        println!(
            "| {} | {} | {} | {} | {} | {} | {:.1} | {:.2} |",
            r.name, r.empty, r.portals, r.rooms, r.bytes, r.split_faces, r.csg_ms, r.rooms_ms
        );
    }
}
