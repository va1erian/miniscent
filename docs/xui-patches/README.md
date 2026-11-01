# xui patches

Fixes to the xui dependency (rev `b3c8cd4`, upstream main) that are not upstream
yet. xui is `https://github.com/va1erian/xui`.

History: the earlier patch for the `ScaleFactorChanged` / capture `RefCell`
double borrow was upstreamed (xui#313) and its vendored copy removed. Pointer
lock (xui#314) is upstream and used by the editor without patching.

## 0002-map-f5-to-f12.patch (xui issue #316)

Apply from the xui repo root: `git apply 0002-map-f5-to-f12.patch`.

Root cause of "F5 and F7 do nothing" in the editor: `xui-canvas`
`backend/app/keymap.rs::virtual_key` maps only `NamedKey::F1` to `F4`, so a real
window never raises `KeyDown` for F5 to F12. xui-core defines `Key::F5..F12`
and its shortcut hook (`Ui::on_key`, offered every `KeyDown` before the focused
widget) works, so the offscreen backend, which injects events directly, hides
the bug. The patch adds the missing arms and a unit test.

As a fallback the editor also binds Ctrl+B (build) and Ctrl+R (play). F5/F7 are
covered by an injected-event test in `apps/editor/src/ui/app/keys_tests.rs`.

Not carried over from the old patch: `Shared::request_redraw` cloned the
`winit` window out before calling `request_redraw` instead of calling it with the
`windows` guard live. That was defensive only (winit posts the repaint, it does
not re-enter), so it was dropped with the rest.

## Using it here

miniscent builds against a vendored copy, `vendor/xui-canvas`, activated by the
`[patch."https://github.com/va1erian/xui"]` section in the root `Cargo.toml`.
It is upstream `crates/xui-canvas` at `b3c8cd4` plus only the keymap change; its
path dependencies point at the same git rev, and its integration tests and
examples were dropped.

**Remove it once xui#316 is fixed upstream.** Check with:
`git -C <xui clone> grep -n "NamedKey::F5" origin/main -- crates/xui-canvas/src/backend/app/keymap.rs`
(a hit means fixed). Then bump the `rev` in `apps/editor/Cargo.toml`, delete
`vendor/`, the `[patch]` section and the `exclude = ["vendor"]` line in the root
`Cargo.toml`, and run `cargo update -p xui`.

The vendored `README.md` has its two relative links (`../xui`, `../../README.md`)
rewritten to absolute URLs at the pinned rev, because `tools/check_docs.py`
(CI) checks every tracked markdown file and they do not resolve inside this repo.

To run the vendored crate's own tests (it is excluded from the workspace):
`CARGO_TARGET_DIR=target/vendor-tests cargo test --manifest-path vendor/xui-canvas/Cargo.toml --lib`
(some tests write `target/ui/*.png` under the vendor dir; it is git-ignored).
