# xui-canvas

The cross-platform, software-rendered backend for [xui](../xui). It draws the
portable widgets with [tiny-skia](https://github.com/linebender/tiny-skia) into
an RGBA buffer, so it runs (and can be snapshot-tested) without a platform UI
toolkit.

A `WinitBackend` window can also host a GPU renderer. Install a `GlWidget` with
`WinitBackend::set_gl_content(window, widget)` for the whole client area, or
`WinitBackend::set_gl_content_on(node, widget)` for one pane. Each frame the
backend renders `glow` OpenGL through a `GlSurface` (a `glutin` context on the
winit window) into a texture and composites it through the software painter
model, so GL content is one painter among many:

```rust
impl GlWidget for Visualizer {
    fn paint(&self, canvas: &mut dyn Canvas, bounds: Rect, theme: &Theme) {
        // software fallback: runs when no GL context can be created
    }
    fn paint_gl(&self, gl: &glow::Context, bounds: Rect, theme: &Theme) {
        // issue GL calls into an offscreen framebuffer
    }
    fn gl_teardown(&self, gl: &glow::Context) {
        // free GPU resources while the context is current
    }
}
```

The `unsafe` GL context creation, the `glow` loader and the offscreen readback
live in `src/sys/gl/` and `xui-gpu`'s `sys/`; every other module forbids
`unsafe`.

## Features

| Feature | Default | What it adds |
|---|---|---|
| `winit-backend` | yes | Everything that needs a windowing system: `WinitBackend` (`winit` + `softbuffer`), the optional `GlWidget`/`GlSurface` GPU seam (`glutin`/`glow` over `xui-gpu`), the `arboard` clipboard and the `windows` double-click metrics. |

Turn it off to build the **software painter core alone**:

```toml
xui-canvas = { git = "https://github.com/va1erian/xui", rev = "…", default-features = false }
```

The crate then compiles over `xui-core`, `tiny-skia` and `cosmic-text` only and
exposes [`SkiaCanvas`](src/canvas.rs), [`Surface`](src/lib.rs),
[`measure_text`](src/text.rs) and [`OffscreenBackend`](src/offscreen/) — the
model for a custom backend such as LazyOS's. Nothing from `winit`,
`softbuffer`, `glutin`, `glow`, `arboard`, `windows` or `xui-gpu` enters that
dependency tree; `tests/deps.rs` guards it.

### Fonts from memory

On a target with no system font store (and no file-backed `mmap`), register the
bundled bytes before the shaper is first used on the thread:

```rust
xui_canvas::set_default_font(include_bytes!("../fonts/DroidSans.ttf").to_vec());
xui_canvas::add_font(include_bytes!("../fonts/JetBrainsMono-Regular.ttf").to_vec());
xui_canvas::set_default_family("Droid Sans"); // runs that name no family
```

Once any font is registered this way the shaper loads only those bytes:
`load_system_fonts` is never called and no file is memory-mapped. Invalid bytes
are skipped without panicking.

`TextStyle`'s horizontal alignment is applied per line for natural-width
(non-wrapped) runs, and [`Surface::pixels`](src/lib.rs) borrows the RGBA bytes
so a backend can present a frame without `to_image`'s whole-surface clone.

## Constraints

- **Composited, not a takeover.** GL content is rendered offscreen and composited
  through the same software surface as CPU nodes: window-level content is the
  base layer, node-level content sits at its node's bounds. The readback makes it
  one painter among many rather than a GPU frame over the whole window.
- **Fallback.** When no GL display/config/context can be created (a headless
  session, no driver) the backend paints `GlWidget::paint` on the software
  surface for good, so the window stays usable. The offscreen backend never has
  a GL context and always uses this fallback.
- **Platforms.** `glutin`'s default backends (EGL/GLX/X11/Wayland/WGL) are
  dlopen-based, so the Linux cross-check needs no system graphics headers beyond
  what winit already needs. Context requests are core-profile 3.3.

`cargo run -p xui-canvas --example gl` opens a window with an animated,
GPU-rendered triangle; set `XUI_DEMO_AUTOCLOSE_MS` to have it quit itself.

See the [workspace README](../../README.md) and the
[epic](https://github.com/va1erian/xui/issues/1).
