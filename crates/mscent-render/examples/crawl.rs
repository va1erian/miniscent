//! Prints how much the shadow edges of the showcase level crawl and jag.
//!
//! `cargo run --release -p mscent-render --example crawl [out_dir]`
//!
//! For each rig (a floor shadow edge at near, mid and far distance), internal
//! size (the game's full 640x360, the 480x270 and 320x180 the adaptive
//! controller steps down to) and step (1/16 and 1/4 pixel per frame) it
//! moves the camera for 64 frames and reports luminance noise of fixed world
//! points, crawl of the edge contour and the stair-step of the contour; see
//! `tests/support/crawl.rs`. PNGs (2x nearest, followed points marked, plus
//! an amplified difference of two frames) go to `out_dir`
//! (default `target/shots/crawl`); the followed points are marked only in the `_marked` copies.

#[allow(dead_code)]
#[path = "../tests/support/crawl.rs"]
mod crawl;
#[allow(dead_code)]
#[path = "../tests/support/stability.rs"]
mod stability;

use std::path::Path;

use crawl::{COARSE, FINE, FULL, LOW, Motion, REDUCED, Rig, Run};
use mscent_render::png_encode::encode_png;
use mscent_render::{Framebuffer, Quality};
use mscent_world::showcase_level;
use stability::Look;

fn enlarge(fb: &Framebuffer, marks: &[(f32, f32)]) -> (u32, u32, Vec<u32>) {
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    let mut big = vec![0u32; w * h * 4];
    for y in 0..h * 2 {
        for x in 0..w * 2 {
            big[y * w * 2 + x] = fb.pixels()[(y / 2) * w + x / 2];
        }
    }
    for &(x, y) in marks {
        let (bx, by) = ((x * 2.0) as usize, (y * 2.0) as usize);
        if bx < w * 2 && by < h * 2 {
            big[by * w * 2 + bx] = 0xffff_00ff;
        }
    }
    (fb.width() * 2, fb.height() * 2, big)
}

/// Absolute difference of two frames, 16x amplified.
fn diff(a: &Framebuffer, b: &Framebuffer) -> Framebuffer {
    let mut out = Framebuffer::new(a.width(), a.height()).expect("framebuffer");
    for (o, (&p, &q)) in out
        .pixels_mut()
        .iter_mut()
        .zip(a.pixels().iter().zip(b.pixels()))
    {
        let ch = |s: u32| {
            let d = ((p >> s) & 0xff).abs_diff((q >> s) & 0xff);
            (d * 16).min(255)
        };
        *o = 0xff00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0);
    }
    out
}

fn write(dir: &Path, name: &str, fb: &Framebuffer, marks: &[(f32, f32)]) {
    let (w, h, px) = enlarge(fb, marks);
    let png = encode_png(w, h, &px).expect("encode");
    std::fs::write(dir.join(format!("{name}.png")), png).expect("write png");
}

fn dump(dir: &Path, rig: &Rig, run: &Run, tag: &str) {
    let (first, last) = (&run.frames[0], &run.frames[2]);
    // The followed points are magenta pixels: kept out of the plain frame so
    // they are not mistaken for dotted artifacts in the shading.
    let name = format!("{}_{tag}", rig.name);
    write(dir, &format!("{name}_first"), first, &[]);
    write(dir, &format!("{name}_first_marked"), first, &run.points[0]);
    write(
        dir,
        &format!("{}_{tag}_diff", rig.name),
        &diff(first, last),
        &[],
    );
}

fn main() {
    let out = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .unwrap_or_else(|| "target/shots/crawl".into()),
    );
    std::fs::create_dir_all(&out).expect("create output dir");
    let level = showcase_level();
    for look in [Look::Flat, Look::Real] {
        let r = stability::build(&level, look);
        let q = if look == Look::Flat {
            Quality::Low
        } else {
            Quality::High
        };
        let name = format!("{look:?}").to_lowercase();
        println!("== {look:?} {q:?}: lum = levels, contour/saw = pixels");
        for rig in crawl::rigs() {
            for (tag, size) in [("full", FULL), ("s750", REDUCED), ("s500", LOW)] {
                for (step_name, px) in [("1/16px", FINE), ("1/4px", COARSE)] {
                    let c = crawl::measure(&r, &rig, q, size, px).expect("measure");
                    // Texture detail (stripes, grout, mortar) crosses the profile
                    // and reads as a zigzag, so the stair-step is only a shading
                    // measure on the flat look.
                    let saw = if look == Look::Flat {
                        format!("saw {:.3} (pp {:.2})", c.saw_px, c.saw_pp)
                    } else {
                        "saw n/a (texture detail)".to_string()
                    };
                    println!(
                        "{:<10} {tag:<5} {step_name:<6} lum {:.2} (edge {:.2})  contour {:.3} (max {:.3})  {saw}  [{} pts, {} samples]",
                        rig.name,
                        c.lum_rms,
                        c.lum_rms_edge,
                        c.contour_px,
                        c.contour_max,
                        c.points,
                        c.contour_samples
                    );
                }
                let run = crawl::run(&r, &rig, Motion::Strafe, q, size, COARSE, true).expect("run");
                let tag = format!("{name}_{tag}");
                dump(&out, &rig, &run, &tag);
            }
        }
    }
}
