# Agent guide

Miniscent is a Rust workspace (see [docs/plan.md](docs/plan.md) and
[docs/work-items.md](docs/work-items.md)). Contributors, human or agent, follow
[CONTRIBUTING.md](CONTRIBUTING.md). The rules that matter most:

- Treat level files and all disk input as untrusted: validate lengths and
  ranges, use checked arithmetic, fail closed, never panic.
- `unsafe` is minimal and every block has a `// SAFETY:` comment.
- Small single-purpose functions, comments that explain why, no dead code.
- Files stay under 500 lines (`python tools/check_file_length.py`).
- Evidence over assertion: write tests that read real output and run them.
- Performance is a feature; measure before and after.
- Tolerances live only in `mscent_math::epsilon`.
- The software and hardware renderers stay on par. A change to what a frame looks
  like lands in both in one PR, with a row in `crates/mscent-gpu/tests/parity.rs`.
  New looks go in both or neither. Shared constants are never retyped in WGSL. Do
  not loosen a parity limit to pass a test ([docs/hardware-renderer.md](docs/hardware-renderer.md)).

Gate before pushing:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -D clippy::undocumented_unsafe_blocks
cargo test --workspace
python tools/check_file_length.py
python tools/check_docs.py
```

Run it (details in [docs/status.md](docs/status.md)):

```
cargo run --release -p miniscent-game
cargo run --release -p miniscent-game -- --screenshot target/shots/p.png --script apps/game/scripts/flythrough.txt --shots-at 2,12,24
cargo run --release -p mscent-editor
cargo run --release -p mscent-editor -- --selftest target/shots/editor
```

Crate edges: world depends on math; collide on math and world; render on math
and world; game on all of them; texgen on math, and render on texgen too (it makes materials from recipes); gpu on render, and on wgpu; script depends on nothing, and game on script too; the game talks to the OS only through `mscent-platform` and never depends on xui (only `apps/editor` does). Do not add external dependencies without a
reason recorded in the PR.
