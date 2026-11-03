use mscent_math::rng::Rng;

use super::*;

const STOCK: [&str; 3] = [
    include_str!("../../../../assets/enemies/sentry.msb"),
    include_str!("../../../../assets/enemies/dart.msb"),
    include_str!("../../../../assets/enemies/mantis.msb"),
];
const HEAD: &str = "miniscent-blueprint 1\nname a\nclass drone\nskin hull metal\n";

/// Whatever loads must also build or refuse, with nothing left to go wrong.
fn try_everything(bytes: &[u8]) {
    let Ok(b) = read(bytes) else {
        return;
    };
    let Ok(m) = build(&b) else {
        return;
    };
    for mesh in m.parts.iter().flat_map(|p| &p.lods) {
        let n = mesh.vertices.len();
        assert!(mesh.triangles.iter().flatten().all(|&i| usize::from(i) < n));
        assert!(mesh.roles.iter().all(|&r| usize::from(r) < b.skins.len()));
    }
}

#[test]
fn flipped_bits_never_panic() {
    let mut rng = Rng::new(0xB10);
    for text in STOCK {
        for _ in 0..150 {
            let mut bytes = text.as_bytes().to_vec();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(bytes.len() as u32) as usize;
                bytes[i] ^= 1 << rng.below(8);
            }
            try_everything(&bytes);
        }
    }
}

#[test]
fn every_truncation_is_handled() {
    for text in STOCK {
        for end in (0..text.len()).step_by(29) {
            try_everything(&text.as_bytes()[..end]);
        }
    }
}

#[test]
fn changed_digits_never_panic() {
    // Bit flips mostly break the syntax. This keeps it, and moves the numbers.
    let mut rng = Rng::new(0xD16);
    for text in STOCK {
        for _ in 0..150 {
            let mut bytes = text.as_bytes().to_vec();
            for _ in 0..8 {
                let i = rng.below(bytes.len() as u32) as usize;
                if bytes[i].is_ascii_digit() {
                    bytes[i] = b'0' + rng.below(10) as u8;
                }
            }
            try_everything(&bytes);
        }
    }
}

fn refused_as_costly(body: &str) {
    let b = read(format!("{HEAD}{body}").as_bytes()).unwrap();
    assert!(
        matches!(build(&b), Err(ModelError::TooCostly { .. })),
        "{} ms",
        estimated_ns(&b) / 1_000_000
    );
}

#[test]
fn smoothing_a_whole_box_many_times_is_refused() {
    let smooths = "smooth sphere 0 0 0 12\n".repeat(MAX_PIECES - 1);
    refused_as_costly(&format!(
        "part p cell=0.25\nadd sphere 0 0 0 8 hull\n{smooths}"
    ));
}

#[test]
fn many_chunks_of_surface_are_refused() {
    let adds: String = (0..MAX_PARTS)
        .map(|i| format!("part p{i} cell=0.5\nadd sphere 0 0 0 20 hull\n"))
        .collect();
    // Either limit will do, as long as nothing starts the work.
    match read(format!("{HEAD}{adds}").as_bytes()) {
        Ok(b) => assert!(matches!(build(&b), Err(ModelError::TooCostly { .. }))),
        Err(e) => assert!(e.to_string().contains("chunks"), "{e}"),
    }
}

#[test]
fn csg_at_its_brush_limit_is_refused() {
    let mut body = String::from("part p\n");
    for i in 0..MAX_CUT_BRUSHES - 1 {
        body += &format!("solid cube {} 0 0 1 1 1 hull\n", i % 8);
    }
    body += "cut cube 0 0 0 1 1 1\n";
    refused_as_costly(&body);
}

#[test]
fn stock_models_are_within_the_limit() {
    for text in STOCK {
        let b = read(text.as_bytes()).unwrap();
        assert!(estimated_ns(&b) < MAX_MODEL_NS);
    }
}
