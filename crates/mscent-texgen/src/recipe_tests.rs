use super::*;
use mscent_math::rng::Rng;

const SMALL: &str = "miniscent-recipe 1
size 64 32
gloss 25
# a comment, and a blank line

op 0 0 1 noise freq_x=8 persistence=0.25 mode=ridged color1=1,0.5,0
op 1 0 1 flat color=0.5,0.5,0.5,0.25
op 0 1 2 merge mode=multiply
op 0 2 1 store name=a%20b
op 0 3 1 out-albedo
";

#[test]
fn it_reads_what_the_format_says() {
    let r = read(SMALL.as_bytes()).expect("reads");
    assert_eq!((r.size.width(), r.size.height(), r.gloss), (64, 32, 25));
    assert_eq!(r.blocks.len(), 5);
    let noise = &r.blocks[0];
    assert_eq!(
        (noise.x, noise.y, noise.w, noise.spec.name),
        (0, 0, 1, "noise")
    );
    assert_eq!(r.blocks[2].w, 2);
    let values = r.blocks[3].params.values();
    assert_eq!(values[0], Value::Text("a b".to_string()));
}

#[test]
fn what_it_writes_reads_back_the_same() {
    let r = read(SMALL.as_bytes()).expect("reads");
    let text = write(&r);
    assert_eq!(read(text.as_bytes()).expect("reads back"), r);
    assert!(!text.contains("octaves"), "defaults are left out:\n{text}");
}

#[test]
fn awkward_text_survives_the_round_trip() {
    let store = ops::find("store").expect("store");
    let mut params = Params::defaults(store);
    let name = "a b%c=d#e\u{e9}\t";
    params
        .set(store, "name", Value::Text(name.to_string()))
        .expect("text");
    let r = Recipe {
        size: Size::new(8, 8).expect("size"),
        gloss: 1,
        blocks: vec![Block {
            x: 0,
            y: 0,
            w: 1,
            spec: store,
            params,
        }],
    };
    assert_eq!(read(write(&r).as_bytes()).expect("reads"), r);
}

#[test]
fn hostile_files_are_errors() {
    let cases = [
        "".to_string(),
        "miniscent-recipe 2\nsize 8 8\n".to_string(),
        "miniscent-recipe 1\n".to_string(),
        "miniscent-recipe 1\nsize 3 8\n".to_string(),
        "miniscent-recipe 1\nsize 8 99999999\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\ngloss 256\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 nosuchop\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 99999 0 1 flat\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat nope=1\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color=2,0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color=1,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color=NaN,0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 noise octaves=999\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 noise persistence=inf\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 noise mode=wrong\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 store name=%zz\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 store name=%ff\n".to_string(),
        format!(
            "miniscent-recipe 1\nsize 8 8\nop 0 0 1 store name={}\n",
            "a".repeat(65)
        ),
        "miniscent-recipe 1\nsize 8 8\nstroke 0 0 0,0,0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat\nstroke 0.1 0 0,0,0,0 0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nsomething else\n".to_string(),
    ];
    for case in cases {
        assert!(read(case.as_bytes()).is_err(), "accepted: {case:?}");
    }
    assert!(read(&[0xff, 0xfe, 0x00]).is_err());
    assert!(read(&vec![b' '; MAX_FILE + 1]).is_err());
}

#[test]
fn too_many_blocks_is_an_error() {
    let mut text = String::from("miniscent-recipe 1\nsize 8 8\n");
    for i in 0..=MAX_BLOCKS {
        text.push_str(&format!("op {i} 0 1 flat\n"));
    }
    assert!(read(text.as_bytes()).is_err());
}

#[test]
fn bit_flips_and_truncations_never_panic() {
    let bytes = SMALL.as_bytes();
    for cut in 0..bytes.len() {
        let _ = read(&bytes[..cut]);
    }
    let mut rng = Rng::new(0x07ec_19e5);
    for _ in 0..20_000 {
        let mut b = bytes.to_vec();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(b.len() as u32) as usize;
            b[at] ^= 1 << rng.below(8);
        }
        let _ = read(&b);
    }
}
