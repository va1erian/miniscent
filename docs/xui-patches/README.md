# xui patches

Fixes to the pinned xui dependency (rev `71dc3b5`) that have not been upstreamed
yet. xui is `https://github.com/va1erian/xui`.

## 0001-fix-scale-factor-borrow.patch

Apply from the xui repo root: `git apply 0001-fix-scale-factor-borrow.patch`.

Root cause: in `xui-canvas` `backend/app/mod.rs`, the `ScaleFactorChanged`
handler read the window's inner size in an `if let ... && let Some(state) =
windows.borrow_mut()` chain. The scrutinee's `Ref` from `windows.borrow()` lives
for the whole chain, so the `borrow_mut()` panicked with "RefCell already
borrowed" whenever a window moved to a monitor with a different scale factor.

Changes (all in `crates/xui-canvas/src/backend`):

- `app/window.rs`: new `adopt_inner_size`, which drops the shared guard before
  measuring and before taking the mutable one. `ScaleFactorChanged` uses it.
  Three unit tests; the re-entrancy one panics with the original message if the
  old if-let chain is put back.
- `app/input.rs` `cursor_moved`: `if let Some(c) = *captured.borrow()` kept the
  guard through delivery of `MouseMove`; a widget calling `set_capture` or
  `release_capture` from that handler (a drag does) hit the same panic. The id
  is now copied out first.
- `mod.rs` `Shared::request_redraw`: clones the `winit` window out instead of
  calling into `winit` with the `windows` guard live.

Audited and left alone: `Resized`, `set_dpi`, `deliver` (clones the sink out
before calling it), `redraw`, `timers`, `focus`/`release_capture` (all drop
their guards before delivering). Not fixed: `gl.rs` teardown and
`render::render_image` call GL widget callbacks with `windows` borrowed
mutably; a widget re-entering the backend from there would still panic.
The input.rs case has no unit test because it needs a live `Shared`, which
needs a winit event loop on the main thread.

## Using it here

miniscent builds against a vendored copy, `vendor/xui-canvas`, activated by the
`[patch."https://github.com/va1erian/xui"]` section in the root `Cargo.toml`.
Its path dependencies point at the same git rev, and its integration tests and
examples were dropped. Once the fix is merged upstream and the `rev` in
`apps/editor/Cargo.toml` is bumped past it: delete `vendor/`, the `[patch]`
section and the `exclude = ["vendor"]` line in the root `Cargo.toml`, then
`cargo update -p xui`.

The vendored `README.md` has its two relative links (`../xui`, `../../README.md`)
rewritten to absolute URLs at the pinned rev, because `tools/check_docs.py`
(CI) checks every tracked markdown file and they do not resolve inside this repo.

To run the vendored crate's own tests (it is excluded from the workspace):
`CARGO_TARGET_DIR=target/vendor-tests cargo test --manifest-path vendor/xui-canvas/Cargo.toml --lib`
(some tests write `target/ui/*.png` under the vendor dir; it is git-ignored).

## 0002-map-f5-to-f12.patch

Apply from the xui repo root: `git apply 0002-map-f5-to-f12.patch`.

Root cause of "F5 and F7 do nothing" in the editor: `xui-canvas`
`backend/app/keymap.rs::virtual_key` maps only `NamedKey::F1` to `F4`, so a real
window never raises `KeyDown` for F5 to F12. xui-core defines `Key::F5..F12`
and its shortcut hook (`Ui::on_key`, offered every `KeyDown` before the focused
widget) works, so the offscreen backend, which injects events directly, hides
the bug. The patch adds the missing arms and a unit test.

The patch is applied to the vendored `vendor/xui-canvas`. As a fallback the
editor also binds Ctrl+B (build) and Ctrl+R (play), keys that arrived even
before the fix. F5/F7 are covered by an injected-event test in
`apps/editor/src/ui/app/keys_tests.rs`.
