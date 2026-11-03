# Hardware renderer: plan

Status: done. M14.1 to M14.12 are built, the GPU draws the editor, Instant Play and the game, and what was not verified is listed per item in [status.md](status.md); costs are in [perf/hardware.md](perf/hardware.md). Figures
under "Measured" were run for this plan; all others are targets to measure. The work
items are in [work-items.md](work-items.md) as M14.

## Goal

Hold 4K at full quality on a desktop GPU, with the same picture the software
renderer draws, **in the editor's 3D pane first** and then in the game.

**This becomes the primary renderer**, by the owner's decision of 2026-11-02. The
software renderer's main target becomes LazyOS, which has no GPU driver. It stays the
fallback on any machine where the GPU path fails to start, and for now the parity test
still holds the two to one picture.

- **Fast.** Native 4K with every feature on, no upscale.
- **Small.** One crate, a few thousand lines, one dependency.
- **Readable.** A frame reads as a list of passes in the order the software
  renderer fills them, with one small file per pass and one shader per kind of surface.
- **On par.** One test compares the two renderers, and it runs in the `cargo test`
  CI already does ([The parity gate](#the-parity-gate)).

Not in scope:

- New looks. No shadow maps, HDR or post-processing on one side only. A new look
  goes in both renderers or in neither. The GPU may draw finer, by the owner's
  decision of 2026-11-03: more resolution, and at High multisampling and
  anisotropic filtering (`Finish::Fine`, M14.12), as the shading stays the same and
  `Finish::Reference` is still held to software.
- Baking on the GPU. Lightmaps stay CPU work, shared by both renderers.
- OpenGL, D3D12, macOS, the web and LazyOS. A machine without working Vulkan draws in
  software.

## Measured

Showcase level, `--bench --no-adaptive`, release build, on the dev machine (24
threads, RTX 5070 Ti). This is the render call only: the present copy to the window
comes on top.

| Internal size | Low avg | High avg | High p95 | High worst |
|---|---|---|---|---|
| 640x360 | 1.06 ms | 1.29 ms | 1.47 ms | 2.62 ms |
| 1920x1080 | 6.17 ms | 6.72 ms | 8.18 ms | 17.12 ms |
| 3840x2160 | 17.15 ms | 22.21 ms | 23.42 ms | 57.60 ms |

`facility.msl` at 4K High averages 20.07 ms. At 4K the bench's resolution controller
ended at 250 permille, the lowest it goes. So 24 threads miss 60 fps at 4K before the
present, and the frame's cost grows nearly with its pixel count, which more CPU
optimisation will not fix. The reference i3-7100U has 4 threads and was not measured.
The `voxel-*.msl` levels have no player start, so `--bench` cannot run them.

Voxel levels on the GPU (M14.5), through `examples/gpu_shot.rs` from the poses
`voxel_shot` uses, High, release, median of 60 render calls including the colour
readback, against software's `render` at the same size:

| Level, voxel triangles | 1920x1080 GPU | 1920x1080 software | 3840x2160 GPU | 3840x2160 software |
|---|---|---|---|---|
| `voxel-mushrooms.msl`, 6,759 | 1.23 ms | 7.35 ms | 4.88 ms | 23.64 ms |
| `voxel-coral.msl`, 20,480 | 1.28 ms | 8.25 ms | 4.96 ms | 24.10 ms |
| `voxel-cave.msl`, 24,184 | 1.29 ms | 5.93 ms | 5.04 ms | 18.57 ms |

The same frames with the voxel draw taken out read 4.81 ms (cave) and 4.99 ms (coral)
at 4K, so the voxel pass itself is lost in the noise of the readback, which is most of
the 4K figure. A new scene of coral and its first frame take 7.4 to 8.2 ms at 1080p,
against 10.6 to 11.4 ms for the showcase, which has more faces. The first voxel scene
on a device also compiles the voxel pipeline, about 4 ms, kept for the device's life.

Cost of the abstraction: the same probe for each library, with one pipeline, a sampled
texture, a full-screen draw and a readback, built with the workspace's release profile.
Each drew offscreen at 3840x2160 and read back the expected pixel on this machine.

| Library and backends | Probe binary | `cargo tree` lines | Game, estimated |
|---|---|---|---|
| blade-graphics 0.9.0, Vulkan | 2,027,008 bytes | 66 | 3.9 MB |
| wgpu 30.0.1, D3D12 (for comparison) | 3,166,720 bytes | | 5.1 MB |
| **wgpu 30.0.1, Vulkan (chosen)** | 3,418,112 bytes | 88 | 5.3 MB |
| wgpu 30.0.1, Vulkan + GL (for comparison) | 3,919,360 bytes | 93 | 5.8 MB |

The estimate is today's game, 2,033,152 bytes against a 2.2 MB budget, plus the probe
less an empty program under this profile, about 119 KB ([status.md](status.md)). Tree
lines count a crate twice where cargo marks a repeat, so they compare, not count.

The editor's route: the wgpu Vulkan probe, drawing at 4K, copying to a buffer, mapping
it and copying into a CPU array, took a **median 4.51 ms** over 40 frames, worst 31.15
ms. Its draw is trivial, so this is close to the pure cost of a readback. Real shaders
add their GPU time, and depth adds a second copy when it is read.

## The abstraction: wgpu on Vulkan

Decided by the owner: wgpu, with Vulkan as its only backend.

| Option | Backends | Verdict |
|---|---|---|
| **wgpu** | Vulkan here; it also offers GL/GLES, D3D12, Metal | **Chosen.** Safe API, validation that turns misuse into an error, no window or game framework. MSRV 1.87 against our 1.98 |
| [blade-graphics](https://github.com/kvark/blade) | Vulkan on Windows and Linux | **The close alternative.** 1.39 MB smaller than the chosen build. Not taken, see below |
| miniquad | GL, GLES, Metal; no Vulkan | Small, but it owns the event loop, which would displace winit and the `Platform` trait |
| SDL3 GPU | Vulkan, D3D12, Metal; no OpenGL | A C library and a build step, and more framework than wanted |
| sokol_gfx, bgfx | Mainly GL, D3D, Metal | C or C++ builds |
| `ash` plus `glow` by hand | Vulkan, GL | Two renderers to write and keep equal. The opposite of small |

The rows for miniquad, SDL3, sokol and bgfx are from memory and were not re-checked or
measured: each fails on backends or build before size matters.

**Why not blade, at 1.39 MB less.** With both on Vulkan, size is the argument for blade
and these are the arguments against:

- **`unsafe` and no error values.** `Context::init` is `unsafe`, so `mscent-gpu` could no
  longer forbid it. Blade's validation is an optional debug layer, and this project's
  first rule is to fail closed. wgpu returns errors a level can trigger and survive.
- **Newer drivers.** Its README requires `VK_KHR_dynamic_rendering` and its FAQ asks for
  fresh drivers. The reference machine's HD 620 was not checked against this.

The build enables `vulkan`, `wgsl` and `std`, with default features off. WGSL stays, on
measurement: on the probe, leaving out every front end saves 2 KB (3,416,064 bytes
against 3,418,112), since wgpu links naga's validator either way, and SPIR-V costs 262
KB more (3,680,256).

Vulkan-only removes the GL constraints: storage buffers are available, so per-face data
can live in one rather than in every vertex. Compute and bindless textures are still
not used, as nothing here needs them. M14.2 checks that a machine with no Vulkan loader
or driver still starts the app and reaches software.

## Architecture

```
crates/mscent-render   software renderer, and everything both renderers read
crates/mscent-gpu      NEW: the hardware renderer; depends on math, world, render, wgpu
```

- `mscent-gpu` keeps `unsafe_code = "forbid"`. wgpu's safe surface API takes an
  `Arc<Window>`, so this crate needs no exception.
- `mscent-render` does not gain a wgpu dependency, and its CPU path stays untouched
  for LazyOS.
- The editor depends on it from M14.4. A `hardware` cargo feature on `apps/game` brings
  it into the game at M14.7, on by default in desktop builds.

### Three seams

**1. One description of a frame.** Today `render_with_effects` takes these as loose
arguments. They become one struct, so a feature added to a frame is visible to both
renderers at the same place:

```rust
pub struct FrameInput<'a> {
    pub view: &'a View,
    pub quality: Quality,
    pub actors: &'a [Actor],
    pub effects: Effects<'a>, // particles, dynamic lights, mover poses
}

pub trait FrameRenderer {
    /// Leaves colour, and depth where `fb` asks for it, in `fb`.
    fn draw(&mut self, fb: &mut Framebuffer, frame: &FrameInput) -> Option<SceneStats>;
}
```

Two real implementations, so this is not speculative. Software fills `fb` as it does
now. Hardware draws on the GPU and reads back into `fb`, so **everything already written
against a `Framebuffer` keeps working unchanged**: the HUD, menus and notices, the white
flash, the editor's highlights and banners, the pane's checksums and the selftests. The
game's direct route to the screen is an addition to this (M14.7), not a replacement.

**2. Assets and visibility apart from the rasterizer.** `LevelRenderer` splits into
`LevelAssets` (scene, lightmap, mips, UV maps, materials, spec tables, lighting) and the
software frame state. A renderer owns its assets, as voxel edits change them between
frames, and one active renderer at a time needs no sharing: `into_assets` and
`LevelRenderer::from_assets` hand a level from one to the other. `Scene::visible_set` is
the flood and walk, which both use, and clipping and projection, which only software
needs, stay in `list_frame_voxels`. The GPU therefore culls with the same portals, so what is in view
cannot disagree.

**3. A window for the GPU.** Needed only by M14.7. `Platform` gains one method:

```rust
/// The window to render into, for a backend that can. Headless returns None.
fn gpu_window(&self) -> Option<Arc<dyn GpuWindow>> { None }
```

`GpuWindow` is `HasWindowHandle + HasDisplayHandle + Send + Sync`, from
`raw-window-handle`, which is already in the lockfile through winit. The game still
never names a windowing library, and `mscent-platform` never names wgpu. The softbuffer
surface must be created on first `present`, so that it never fights a swapchain on the
same window.

### A frame

The passes are the software renderer's order, and read as such:

```rust
fn draw(&mut self, fb: &mut Framebuffer, frame: &FrameInput) -> Option<SceneStats> {
    let visible = self.assets.visible(frame.view);       // same flood as software
    self.upload.frame(&visible, frame);                  // one ring buffer per frame
    let mut pass = self.targets.scene_pass(&mut encoder);
    sky::draw(&mut pass, &self.sky);
    movers::draw(&mut pass, &self.movers, frame.effects.poses);
    voxels::draw(&mut pass, &self.voxels, &visible);
    faces::draw(&mut pass, &self.faces, &visible, frame.quality);
    actors::draw(&mut pass, &self.actors, frame.actors);
    particles::draw(&mut pass, &self.particles, frame.effects.particles);
    drop(pass);
    self.targets.read_back(encoder, fb)
}
```

| GPU file | Software counterpart | Shader |
|---|---|---|
| `device.rs`, `targets.rs` | `framebuffer.rs`, `tiles.rs` | |
| `sky.rs` | `clear_sky` | `sky.wgsl` |
| `faces.rs`, `textures.rs` | `level/fill.rs`, `lighting_shader.rs`, `light_sampler.rs`, `bump.rs`, `specular.rs` | `face.wgsl` |
| `movers.rs` | `level/movers.rs` | `face.wgsl` with a pose |
| `voxels.rs` | `voxel/` | `voxel.wgsl` |
| `models.rs` (M15.11) | `model/` | `voxel.wgsl`, in model space |
| `actors.rs` | `mesh.rs` | `mesh.wgsl` |
| `particles.rs` | `particles/` | `sprite.wgsl`, `decal.wgsl` |
| `vistas.rs`, `holes.rs` | `level/vista.rs`, `scene/holes.rs` | `face.wgsl` per layer, `hole.wgsl` |
| `overlay.rs` (M14.7 only) | `draw2d.rs` (used, not replaced) | `blit.wgsl` |

Target: under 3,000 lines of Rust and 800 of WGSL, no file over 300 lines.

### Choices that make parity cheap

Each of these carries a software fact over unchanged, so there is less to reconcile:

| Fact | GPU choice |
|---|---|
| Depth is `1/z`, 0.0 means nothing drawn, the test is `iz > depth` | `Depth32Float`, cleared to 0.0, compare `Greater`, with depth running from 1 at near to 0 at far and linear in `1/z`. This is reversed Z, already the best GPU layout, and clips at far as software does. Reading it back is one division (`Camera::inv_z_of_gpu_depth`) |
| Colour maths is in display space, on 8 bits, and 256 means full light | A non-sRGB target, so the hardware does no conversion. Light in `Rgba16Float` on the same 256 scale |
| Mips are box-filtered by `mips.rs` and `lightmap_mips.rs` | Upload those levels, so there is no second mip generator and the GPU's own is never called |
| `dither_of` is an integer hash of the screen position | The same hash in WGSL, on `u32` |
| `KNEE`, `SPEC_GAIN`, `BUMP_FULL`, `EMISSIVE`, `UNLIT` | **Never retyped.** Passed from the Rust constants as pipeline overrides |
| `SpecTable` is a power table per gloss | The table's entries in a storage buffer, read with its interpolation, not `pow()` |
| Voxel axis chosen per pixel from the normal, with a seam band | Same formula: a fragment shader suits this better than spans did |
| No anisotropic filtering | Samplers set to match, with the same `texel_rate` formula for level of detail where derivatives disagree |

### Draws

- One static vertex buffer for all faces: position, texture UV, luxel UV, face index. A
  storage buffer holds each face's tangent frame.
- One index ring. Each frame the visible faces are written grouped by material, so
  there is one draw per material and light mask, a few dozen in a typical frame.
- One bind group per material. Materials differ in size, so texture arrays would not
  help, and a few dozen binds a frame do not justify bindless.
- Dynamic lights: at most `MAX_DYN_LIGHTS` in a uniform. `reaches_face` and
  `reaches_leaf`, already computed on the CPU, choose the mask, so light stays out of
  rooms it cannot reach, exactly as in software.
- Each voxel chunk keeps its three levels of detail in two storage buffers of its own,
  and the shader pulls its vertices from them, so every corner sees its whole
  triangle: whether it is plain, its three looks, and the mip level software takes
  once per triangle. Which chunks, at which level, is software's own `VoxelList`. A
  chunk's mesh and light are shared `Arc`s, so a sculpt between frames, through the
  existing `apply_voxels`, remakes only the chunks whose pointers changed.
- A blended triangle can carry three materials, which one bind group per material
  cannot draw. Voxel texels therefore live in one storage buffer, only for the
  materials in use, and are filtered with software's integer bilinear weights on the
  same box-filtered mips: no device feature, and the same texel to the bit.
- Actors keep one vertex buffer.

No depth pre-pass at first. If overdraw shows in the measurements, the flood's
front-to-back order can be kept within each material's indices.

## The editor's 3D pane

This is the first user of the GPU path, ahead of the game's own window, and it needs no
change to xui.

**How the pane works today.** A render thread owns a `Scene` and returns a
`&Framebuffer` from `Scene::render`. The thread converts it to RGBA bytes
(`studio/worker.rs`), a one-slot mailbox hands them over, and the UI thread paints them
through `View3d::set_rgba`. The UI never waits for a frame.

**The change is on that thread alone.** `LevelScene::render` draws through a
`GpuView` (`studio/gpu_view.rs`) when it can, and the mailbox, the pane and the UI
thread do not change. `GpuRenderer` borrows the level each frame instead of owning it,
as the session owns it and Play draws it in software until M14.6. wgpu needs
no window here because it draws offscreen. Highlights, banners and picking keep working:
`highlight::draw` reads the depth in `fb`, so depth is read back on frames that have
highlights, and only those. Checksums, `frame_pixels` and `--selftest` read the same
pixels they always did.

**Why not xui's `GlWidget`.** It exists, but per xui's `docs/backends.md` it reads back
and composites through the software surface too, so it would not save the copy. It
paints from the UI thread, which would break the rule that a slow scene never slows the
UI, and it is OpenGL through `glow`, a second renderer. The render thread does the same
job with the code the game will use.

**One device, many scenes.** A new scene replaces the old one on every edit, on one
device for the life of the editor, and uploads the level again. Measured on the
showcase, a new scene's first frame at 1080p costs 6.8 ms, about 5 ms over an ordinary
one, so nothing needs caching yet. Opening the device costs about 230 ms, once.

**Size of the frame.** Software caps the pane's frame at 960 pixels wide and scales up.
While the GPU is drawing, the frame is the pane's own size, which also takes xui's
unscaled path for the image.

**Fallback.** Hardware is on by default in the editor, with `--software` and a View menu
command to switch, which is not remembered between runs. No adapter or a failed device
means software with the reason in the log panel. Viewports, vistas and cutouts draw on
the GPU from M14.11. Voxel levels draw on the GPU from M14.5. Until particles land (M14.6), Play stays software. Tests stay on
software unless they ask, as they read exact pixels.

**The paint (M14.10).** The UI thread paints each frame through xui's software canvas.
At 4K that cost 175 to 182 ms a frame, on both renderers, and set the pane's frame
rate. Painting the 3D pane itself took 8.5 ms of that. The rest went on the three
2D views around it, which sit 5 px from the pane. xui repaints a margin of 8 px around
every change, for painters that draw past their bounds (an outline, a glow). That
margin took in each 2D view, whose painter then ran in full: 45 to 67 ms each, mostly
long grid lines, rasterised whole even where a strip of 3 px was all that was kept.

Two changes in xui fix it, merged as [va1erian/xui#319](https://github.com/va1erian/xui/pull/319):

- **`NodeSpec::contained()`**: a node whose painter stays inside its bounds gets no
  margin, in what it reaches or in the damage it causes. The 3D pane and the 2D views
  are contained, so a new frame repaints the pane and nothing else.
- **Shapes that miss the repainted rectangle are skipped**, which changes no pixel; xui's
  region tests, which compare a repaint with a whole-window paint pixel for pixel, pass.
  Painting that repaints a strip of a busy painter costs nearer the strip.

Measured with `--ui-timing target/shots/timing WxH`, release, offscreen backend, a new
frame through the pane's own path and then a picture of the window. The picture copies
the whole window, which a real window does not, so "painting" below is the frame less
that copy, from xui's own timing:

| Window | Before | After | Of which painting |
|---|---|---|---|
| 1280x800 | 26 ms | 2.0 ms | about 1 ms |
| 1920x1080 | | 4.2 ms | about 2.3 ms |
| 3840x2160 | 175 to 182 ms | 17.7 to 20.8 ms | 11.5 ms median, 12.3 ms at the 95th percentile |

Skipping missed shapes alone took the 4K paint from 172 ms to 57 ms. One frame in 300 at
4K took 134 ms. The windowed backend repaints by the same compositor and damage, then
converts the damaged rows for `softbuffer`; that path was not run here.

**The finish (M14.12).** At High, the GPU draws more finely than software can afford:
4x multisampling on edges and trilinear, 16x anisotropic filtering of the materials
(`Finish::Fine`, `crates/mscent-gpu/src/finish.rs`). The shading is the same.
`Finish::Reference` is software's picture, which the parity test holds the GPU to; the
pane and `gpu_shot` use `Finish::of(quality)`. A multisampled depth is read back
through a resolve pass, the mean of the samples that hit something, which on a surface
is the pixel centre's depth as software stores it.

`tests/finish.rs` holds the fine frame to the reference in the mean (1.9 levels at
most, against a limit of 3), and to a truth, the same view drawn four times larger
each way and averaged down. Fine is 4% to 16% nearer the truth over the frame, and 21%
to 36% nearer on silhouettes. Without anisotropic filtering it is further from the
truth than the reference, and without multisampling its silhouettes gain 1% to 8% on
two of the three views, so each half of the finish fails the test when taken out. On
the RTX 5070 Ti a 4K frame costs 5.12 ms fine against 4.92 ms reference, readback
included.

Voxels (M14.5) are multisampled the same way, with a voxel pipeline compiled for each
finish. Their texels sit in a storage buffer, filtered by software's own integer
weights, so they get no anisotropic filtering: a fine voxel frame is the reference
with smoothed edges. On four voxel views its silhouettes come 22% to 31% nearer the
truth (`tests/finish.rs`). The room's wide view, whose edges are mostly brick walls,
comes 9% nearer, and 5% with the voxels taken out, so there the faces' edges gain
less than elsewhere, not the voxels'. Filtering voxel texels in a texture is not done.

**Viewports (M14.11).** Software draws a vista last, through a mask of the viewport
faces' pixels, on a second depth plane. The GPU draws it first: each slot in view gets a
picture of its own, the gradient and then its layers far to near, each layer a render
pass with its depth cleared, scissored to the screen rectangle its faces cover. The
viewport faces then draw last of the level's solid things, at their true depth, each
showing its slot's picture at its pixel. That leaves the same pixels, and a slot hidden
behind a nearer one's face loses to it the same way. Which layers, with which camera,
fog and pan, and which faces each lists, is software's own (`LevelAssets::vista_views`),
so the two cannot disagree. A layer's faces are the face shader with a `Frame` and pose
of its own: its camera and fog, no dynamic lights, and the pan, which slides a self-lit
face's texels in the vertex shader as `scene/pan.rs` slides them. A cutout's coverage
sits in its albedo's alpha, with the alpha plane's own mips, and the material draws
twice: its soft texels first, blended without depth, then its solid ones with depth.
Soft first, as software draws front to back, so a face behind a soft texel covers it in
both; two soft texels over each other can still blend in another order. A layer's
voxels are the level's chunks listed from its eye, with the budgets and detail levels
software gives them (`LevelAssets::vista_voxels`), drawn before its faces with its
frame. The face, cutout and viewport pipelines are compiled once per device, as the
voxels' are: without that, a new scene of a vista level cost the editor's pane more than
its 12 ms and the adaptive scale shrank its frame.

## Two traps in the game path

Neither touches the editor's edit view, as it has no particles and reads back anyway.

**Particles read last frame's depth.** `self.fx.collect(&cam, fog, Some(&fb), ...)` uses
the previous framebuffer to estimate what is hidden, and that sets which particles the
pixel budget thins. Readback keeps this working (M14.6). The direct route (M14.7) has no
CPU depth, and a late GPU readback would depend on timing. There, both renderers take the
estimate from one source: the visible faces rasterized depth-only into a small CPU
buffer, with the existing rasterizer. This can move a software frame that is over budget,
so the change is measured and recorded. The half-res particle layer (M9.6) stays a
software cost saver, and parity cases for particles run under budget.

**2D draws into the frame's own pixels.** On the direct route there is no `fb` to draw
on, and a second 2D renderer is not wanted. The HUD, feedback, notices and menus draw
into a transparent CPU layer through the existing `draw2d`, the bounding box of what
changed is uploaded, and a blit puts it over the scene. The white flash becomes a uniform
on that blit. M14.7 checks that nothing there reads the pixels beneath.

## Failure and untrusted input

Level data is validated before it reaches here, but the GPU adds new limits:

- wgpu's default for an uncaptured error is to panic. `mscent-gpu` installs a handler
  and uses error scopes around building level resources, so an error is a value.
- Counts and sizes are checked against the adapter's limits, using checked arithmetic.
  A level that does not fit returns an error naming the limit, and the app falls back to
  software. A 1024x8192 atlas exceeds wgpu's WebGL2 default limit of 2048, so the device
  must request what the adapter really offers.
- No adapter, no surface format or a failed device: log the reason, draw in software.
- Device lost at run time: rebuild once from `LevelAssets`, and if that fails, continue
  in software. The simulation does not notice.
- A test runs the `hostile_level` corpus through the GPU build path.

The game takes `--renderer auto|software|hardware`. `software` until M14.8, `auto`
after. `hardware` fails with its reason instead of falling back, for tests and bug
reports. A headless run (`--screenshot`, `--script`, `--bench`) stays `software` unless
asked, as its pictures are to be the same on every machine.

## The parity gate

Simple on purpose: one test file, no new crate, script or CI job.

### The test

`crates/mscent-gpu/tests/parity.rs` holds a table of about 15 cases, one per feature,
and each draws at 320x180 on both renderers:

```rust
const CASES: &[Case] = &[
    Case { name: "fog", level: SHOWCASE, pose: FAR_HALL, quality: High, without: NoFog },
    Case { name: "specular", level: SHOWCASE, pose: METAL_WALL, quality: High, without: Quality(Medium) },
    Case { name: "dynlight", level: SHOWCASE, pose: RED_ROOM, quality: High, without: NoLights },
    // ...
];
```

The features: sky, fog, texture, mips, lightmap, light mips, emissive, bump, specular,
actor, mover, and voxel surfaces with their shadows, bump, specular, blends, spots,
detail levels and mips so far, then dynamic light, sprite, decal and depth test. A feature adds a row when it is ported, and a new feature adds one in the PR that
makes it.

Each case makes two checks:

1. **Hardware against software.** After excluding pixels on an edge, the mean error per
   channel and the worst 8x8 block mean must be under one pair of limits, 1.5 and 3
   levels, which live in `mscent_math::epsilon` with every other tolerance. The block
   measure catches a shadow or highlight that moved, which a global mean would average
   away. Each colour is compared with its nearest within one pixel, as software corrects
   perspective every 16 pixels and a GPU at each one: a texel on a steep wall lands up to
   a pixel apart, and past that on a very steep one, with both right.
2. **The case can fail.** The same frame in software with the feature removed must
   *fail* that comparison. Otherwise the case is not looking at its feature. The repo has
   met this before: `examples/crawl.rs` exists because `stability` passed on a crawling
   shadow. It costs only a small software draw, and no test hook ships.

There are no stored images. Each run draws both from current code, so a change to one
side fails until the other follows. On failure the test writes `sw.png`, `hw.png` and
`diff.png` to `parity/` under cargo's test directory, `target/tmp`, and prints the path.

One further test asserts that `ParticleDraws` come out byte-identical. Nothing else is
needed for the simulation, since `Game::step` does not read the renderer.

### In CI

No new job. Two lines go in the existing `check` job in
[ci.yml](../.github/workflows/ci.yml), since `cargo test --workspace` already runs the
test. Runners have no GPU, so it uses Mesa's lavapipe, a CPU Vulkan driver:

```yaml
      - run: sudo apt-get update && sudo apt-get install -y mesa-vulkan-drivers
      - run: cargo test --workspace
        env:
          MSCENT_REQUIRE_GPU: "1" # no adapter is a failure here, a skip elsewhere
```

On `ubuntu-latest` the package takes 9 s. In a `rust:1.98` container on the dev machine
the crate and its dependencies built and ran its tests in 32 s, without real shading. The
target is under 2 minutes added to a cold run and 1 to a cached one. If it costs more or
proves flaky, the fallback is to take those lines out and rely on the local run below.
Parity must not be what makes CI slow.

### Locally

The same `cargo test` uses your real GPU, with nothing to remember. That also covers what
Mesa cannot see, as it is not an Intel or NVIDIA driver. GPU speed is not gated: M14.9
records it in `docs/perf/`, and software keeps `check_frame_cost.py`.

### What was left out

To save time, these are not built: a separate workflow, a checking script, a feature table with a ratchet, a rule failing PRs that touch one side,
a rule on loosening tolerances, 4K comparison and artifact upload. The trade: a one-sided
change that no case happens to show gets past CI. The reviewer agent looks for that at
pre-PR review, at no CI cost. If such drift actually happens, add the rule then.

### The agents

**`AGENTS.md`** is read by every contributor, including agents other than Claude, so the
rule goes there:

```
- The software and hardware renderers stay on par. A change to what a frame looks
  like lands in both in one PR, with a row in crates/mscent-gpu/tests/parity.rs.
  New looks go in both or neither, except that the GPU may draw finer
  (Finish::Fine) while Finish::Reference stays on par. Shared constants are never
  retyped in WGSL. Do not loosen a parity limit to pass a test.
```

The crate edges paragraph gains `mscent-gpu`.

**`.claude/agents/`** gets two thin definitions that point here and do not restate it:

| Agent | Tools | When | Job |
|---|---|---|---|
| `render-parity-reviewer` | Read-only, plus running the test | Pre-PR review of a diff touching `mscent-render` or `mscent-gpu` | Reports a look changed on one side, a retyped constant, a feature with no row, or a `without` that does not remove its feature |
| `render-parity-porter` | Edit, run | After a change lands on one side | Ports it to the other and adds the row. May not edit `epsilon.rs`: if it cannot match, it stops and reports the pictures |

The porter's restriction matters most: left unconstrained, the shortest path to a passing
test is a looser limit.

## Size

`tools/check_size.py` measures two builds in the existing `size` job:

| Build | Budget | Why |
|---|---|---|
| `--no-default-features --features desktop` (software only) | 2.2 MB, unchanged | LazyOS and the original constraint |
| Default (with `hardware`) | Set when the game first links `mscent-gpu`, at M14.7; about 5.7 MB expected | Recorded in that PR, as the script's header requires. Until then the game does not link it, and a budget would measure nothing |

The editor has no size budget, and already carries more than this.

## Costs and budget

Targets, on the dev machine, at 3840x2160, High:

| Step | Target |
|---|---|
| Editor pane: a new scene's first frame, 1080p | 6.8 ms measured; software's edit to picture is 15 ms |
| Editor pane: frame including readback | 5.6 ms measured, 11.7 ms with a highlight |
| Editor pane: a new frame painted by the UI thread | 11.5 ms median measured offscreen, against 16 ms; was 175 to 182 ms |
| Game, direct route: CPU side of a frame | Under 1.5 ms |
| Game, direct route: GPU side | Under 4 ms |
| Game, direct route: worst frame in the flythrough | Under 8.3 ms (120 Hz) |
| Level load added by GPU upload | Under 100 ms, with a warm bake |
| Allocations per frame | No more than the software path's counts in `tests/frame_alloc.rs` |
| Input latency | `desired_maximum_frame_latency` of 1 where supported |

Reference machine: HD 620 at 1920x1080 at 60 fps, unproven until the box is available.
A GPU that cannot hold native size renders smaller and scales up, using the existing
`ResolutionController`.

## Risks

- **Size.** About two and a half times the binary. Accepted by the owner with the choice
  of wgpu.
- **Vulkan only.** No Vulkan driver means no acceleration on that machine, with no second
  GPU path to fall to. Software covers it, slow at 4K but correct. The HD 620's Vulkan
  support on the reference machine has not been tried.
- **The pane's paint.** Fixed offscreen in M14.10, through va1erian/xui#319. A real
  window is not measured.
- **Mesa in CI.** Passes on `ubuntu-latest`. A Mesa update could move a result, and two
  cases sit close to the block limit. The fallback is under "In CI".
- **Tolerance as a loophole.** Filtering, 16-pixel span subdivision against exact
  perspective, and per-span against per-pixel level of detail all differ slightly, so
  limits can't be zero. Mitigated by block error and the failing check. On a very steep
  wall the spans part by more than the pixel allowed, by up to about 4 levels in a block,
  so the mover case stands in a wide room. Copying the spans would make the GPU slower
  and its picture worse, so this is a known difference rather than a bug.
- **Friction.** Every look change now costs twice. It is the point, and the reason the
  Texture Studio, levels and bake are shared and only drawing is duplicated.
- **Driver differences behind one API.** A bug on Intel may never show on Mesa or NVIDIA.
  The software toggle and `--renderer software` are always there.

## Tests

As everywhere: read real pixels.

- Seams: flythrough PNGs and the existing render suite are byte-identical after M14.1, and
  `frame_bench` is unchanged.
- The parity cases, as above. Each shader term is broken by hand once during M14 to see
  its case fail, recorded in the PR, as M10.3 did with its deliberate bugs.
- Editor: `--selftest` passes with hardware on and on the software fallback. A pick, a
  highlight, an edit, a viewport level, a sculpt, and Play.
- Hostile levels through the GPU build: errors, no panic. A forced device loss, and a
  forced limit below the level's atlas, both reach software with the reason logged.
- Resize, minimize and a monitor move in a real window: only the owner can run these,
  and the status doc says so until they do.

## Work items

In [work-items.md](work-items.md).

M14.1 to M14.4 run in order and are the shortest path to an accelerated pane. M14.5 and
M14.6 can then run side by side.

## Later

- **Texture Studio's lit preview** on the GPU, which stays software until something asks.
- **A self-hosted runner** with a real GPU, and 4K in CI, if local runs miss something.
