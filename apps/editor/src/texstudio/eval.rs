//! One evaluation of a page, on the worker: a draft at no more than 128 px, whose
//! images also make the thumbnails, then the full size unless a drag is going on.
//! The draft comes first so that a heavy recipe answers at once, and every block
//! is kept only at that size: a page of 256 blocks at 1024 px would not fit the
//! evaluator's memory limit.

use std::cell::Cell;
use std::sync::atomic::AtomicBool;

use mscent_render::Texture;
use mscent_render::recipe_texture::texture_from_maps;
use mscent_texgen::graph::{self, Cache, Graph, Outcome, RunOptions};
use mscent_texgen::image::Image;
use mscent_texgen::ops::Channel;
use mscent_texgen::pack::{self, byte, normal_strength, normals_from_height};
use mscent_texgen::{Recipe, Size, TexError};

use super::lit::LitTile;

/// Longest side of a draft.
const DRAFT: u32 = 128;
/// Longest side of a thumbnail.
const THUMB: u32 = 48;
/// The normal map of a page with no height.
const FLAT_NORMAL: [u8; 4] = [128, 128, 255, 255];

/// What the preview shows: the selected block, or one map of the finished
/// material. A tab stays chosen when the selection moves, so one map can be
/// watched while editing any block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Block,
    Albedo,
    Height,
    Normal,
    Specular,
    Lit,
}

/// One page to evaluate. Only the newest matters.
pub struct Request {
    /// Increases with every request; a picture answers the request of its number.
    pub seq: u64,
    pub recipe: Recipe,
    pub graph: Graph,
    pub selected: Option<usize>,
    pub tab: Tab,
    /// A drag is going on: the draft is enough, and the next edit is on its way.
    pub draft_only: bool,
    /// The material is hard-coded emissive in the game, so the lit view is flat.
    pub emissive: bool,
}

/// A picture for the preview.
#[derive(Debug)]
pub struct Picture {
    pub seq: u64,
    pub tab: Tab,
    pub width: u32,
    pub height: u32,
    /// RGBA; empty when there is nothing to show and `note` says why.
    pub rgba: Vec<u8>,
    pub note: Option<&'static str>,
    /// At the recipe's own size, not a draft.
    pub full: bool,
}

/// One thumbnail, RGBA.
pub type Thumb = (u32, u32, Vec<u8>);

/// A small picture of each block.
pub struct Thumbs {
    pub seq: u64,
    pub images: Vec<Option<Thumb>>,
}

/// Why a page that compiled did not evaluate.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub seq: u64,
    pub why: String,
    /// The block whose kernel failed, when one did.
    pub block: Option<usize>,
}

/// What an evaluation sends to the UI as it goes.
pub enum Output {
    Picture(Picture),
    Thumbs(Thumbs),
    Failure(Failure),
}

fn rgba(pixels: impl Iterator<Item = [u8; 4]>, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len * 4);
    for p in pixels {
        out.extend_from_slice(&p);
    }
    out
}

fn colours_of(image: &Image) -> Vec<u8> {
    let px = image.pixels().iter();
    rgba(
        px.map(|p| [byte(p[0]), byte(p[1]), byte(p[2]), 255]),
        image.size().texels(),
    )
}

/// Height and specular are data in channel R.
fn greys_of(image: &Image) -> Vec<u8> {
    let px = image.pixels().iter().map(|p| byte(p[0]));
    rgba(px.map(|v| [v, v, v, 255]), image.size().texels())
}

fn normals_of(image: &Image, strength: f32) -> Vec<u8> {
    let normals = normals_from_height(image, strength);
    let channels = normals.iter().map(|n| {
        let [_, r, g, b] = n.to_be_bytes();
        [r, g, b, 255]
    });
    rgba(channels, image.size().texels())
}

/// A thumbnail by nearest texel: it is 48 px of a picture people look at for its
/// gist, and there can be 256 of them per draft.
fn thumb_of(image: &Image) -> Thumb {
    let size = image.size();
    let (w, h) = (size.width().min(THUMB), size.height().min(THUMB));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let p = image.at(
                (x * size.width() / w) as i32,
                (y * size.height() / h) as i32,
            );
            out.extend_from_slice(&[byte(p[0]), byte(p[1]), byte(p[2]), 255]);
        }
    }
    (w, h, out)
}

/// A pass's result as far as the preview goes.
struct Pass<'a> {
    request: &'a Request,
    outcome: &'a Outcome,
    full: bool,
}

impl Pass<'_> {
    fn output(&self, channel: Channel) -> Option<(usize, &Image)> {
        let block = self.request.graph.output(channel)?;
        Some((block, self.outcome.image(block)?))
    }

    fn picture(&self, image: &Image, make: impl Fn(&Image) -> Vec<u8>) -> Picture {
        Picture {
            seq: self.request.seq,
            tab: self.request.tab,
            width: image.size().width(),
            height: image.size().height(),
            rgba: make(image),
            note: None,
            full: self.full,
        }
    }

    /// A one-texel picture of what the game uses for a map the page does not have.
    fn stand_in(&self, texel: [u8; 4], note: &'static str) -> Picture {
        Picture {
            seq: self.request.seq,
            tab: self.request.tab,
            width: 1,
            height: 1,
            rgba: texel.to_vec(),
            note: Some(note),
            full: self.full,
        }
    }

    fn nothing(&self, note: &'static str) -> Picture {
        Picture {
            rgba: Vec::new(),
            width: 0,
            height: 0,
            ..self.stand_in([0; 4], note)
        }
    }

    /// The picture of every tab but the lit one.
    fn flat_picture(&self) -> Picture {
        let r = self.request;
        match r.tab {
            Tab::Block | Tab::Lit => match r.selected.and_then(|b| self.outcome.image(b)) {
                Some(image) => self.picture(image, colours_of),
                None => self.nothing("Select a block to see it here"),
            },
            Tab::Albedo => match self.output(Channel::Albedo) {
                Some((_, image)) => self.picture(image, colours_of),
                None => self.nothing("The page has no out-albedo"),
            },
            Tab::Height => match self.output(Channel::Height) {
                Some((_, image)) => self.picture(image, greys_of),
                None => self.stand_in([0, 0, 0, 255], "No out-height: the surface is flat"),
            },
            Tab::Normal => match self.output(Channel::Height) {
                Some((block, image)) => {
                    let strength = normal_strength(&r.recipe, block);
                    self.picture(image, |i| normals_of(i, strength))
                }
                None => self.stand_in(FLAT_NORMAL, "No out-height: the normals are flat"),
            },
            Tab::Specular => match self.output(Channel::Specular) {
                Some((_, image)) => self.picture(image, greys_of),
                None => self.stand_in([255; 4], "No out-specular: the mask is full"),
            },
        }
    }

    /// The tile the lit view draws, or the note that replaces it.
    fn lit_tile(&self) -> Result<LitTile, &'static str> {
        let r = self.request;
        let maps = pack::pack(&r.recipe, &r.graph, self.outcome)
            .map_err(|_| "The page has no out-albedo")?;
        LitTile::new(maps, r.recipe.gloss, r.emissive, r.seq, self.full)
            .map_err(|_| "These maps do not make a texture")
    }
}

/// Runs a page at `size`, keeping `keep`. `None` when it was cancelled; a failure
/// is sent on its way.
fn run_pass(
    request: &Request,
    size: Size,
    keep: &[usize],
    cache: &mut Cache,
    cancel: &AtomicBool,
    send: &mut dyn FnMut(Output),
) -> Option<Outcome> {
    let failed = Cell::new(None);
    let options = RunOptions {
        cache: Some(cache),
        cancel: Some(cancel),
        keep,
        failed: Some(&failed),
    };
    match graph::run(&request.recipe, &request.graph, size, options) {
        Ok(outcome) => Some(outcome),
        Err(TexError::Cancelled) => None,
        Err(e) => {
            send(Output::Failure(Failure {
                seq: request.seq,
                why: e.to_string(),
                block: failed.get(),
            }));
            None
        }
    }
}

fn draft_size(size: Size) -> Size {
    Size::new(size.width().min(DRAFT), size.height().min(DRAFT)).unwrap_or(size)
}

fn show(pass: &Pass, send: &mut dyn FnMut(Output)) -> Option<LitTile> {
    if pass.request.tab != Tab::Lit {
        send(Output::Picture(pass.flat_picture()));
        return None;
    }
    match pass.lit_tile() {
        Ok(tile) => {
            send(Output::Picture(tile.render(0)));
            Some(tile)
        }
        Err(note) => {
            send(Output::Picture(pass.nothing(note)));
            None
        }
    }
}

/// Evaluates a request, sending what comes of it; the lit tile to animate, if the
/// lit tab is up.
pub fn evaluate(
    request: &Request,
    cache: &mut Cache,
    cancel: &AtomicBool,
    send: &mut dyn FnMut(Output),
) -> Option<LitTile> {
    let full = request.recipe.size;
    let draft = draft_size(full);
    let all: Vec<usize> = (0..request.recipe.blocks.len()).collect();
    let outcome = run_pass(request, draft, &all, cache, cancel, send)?;
    send(Output::Thumbs(Thumbs {
        seq: request.seq,
        images: all
            .iter()
            .map(|&b| outcome.image(b).map(|i| thumb_of(i)))
            .collect(),
    }));
    let pass = Pass {
        request,
        outcome: &outcome,
        full: draft == full,
    };
    let tile = show(&pass, send);
    if request.draft_only || pass.full {
        return tile;
    }
    let keep: Vec<usize> = request.selected.into_iter().collect();
    let outcome = run_pass(request, full, &keep, cache, cancel, send)?;
    show(
        &Pass {
            request,
            outcome: &outcome,
            full: true,
        },
        send,
    )
}

/// A page's finished maps, for a material. Not cancelled by edits: what was asked
/// for is this page, and it is carried in the job.
pub fn bake(recipe: &Recipe, graph: &Graph, cache: &mut Cache) -> Result<Texture, String> {
    let options = RunOptions {
        cache: Some(cache),
        ..Default::default()
    };
    let outcome = graph::run(recipe, graph, recipe.size, options).map_err(|e| e.to_string())?;
    let maps = pack::pack(recipe, graph, &outcome).map_err(|e| e.to_string())?;
    let texture = texture_from_maps(maps).map_err(|e| format!("{e:?}"))?;
    // The mips are made here, not on the UI thread when the table is installed.
    Ok(texture.filtered())
}
