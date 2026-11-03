# Contributing

## Code standards

- **Security first.** Treat every level file, and everything else read from
  disk, as untrusted. Validate lengths and ranges, use checked arithmetic, fail
  closed, and never panic on untrusted input.
- **`unsafe` is minimal** and every block carries a `// SAFETY:` comment.
  Workspace lints forbid `unsafe_code`; a crate that needs it opts out locally.
- **Readable.** Small single-purpose functions, descriptive names, types that
  make invalid states unrepresentable, comments that explain why. No dead code
  and no speculative abstraction.
- **Files stay under 500 lines**, enforced in CI for every `.rs` and `.py` file.
- **Pinned toolchain** in `rust-toolchain.toml`, with `clippy` and `rustfmt`.
- **Evidence over assertion.** Graphics are verified by reading real pixels,
  collision by property tests, performance by the frame-time benchmark.
- **Performance is a feature.** A change that slows the frame or the input
  path is a regression even when correct.

## Before pushing

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -D clippy::undocumented_unsafe_blocks
cargo test --workspace
python tools/check_file_length.py
python tools/check_docs.py
```

## Pre-PR review

After the checks above pass and the branch is committed, run a review of the
diff against `main` before opening the PR. Fix each finding or answer it in the
PR description before merge. A diff touching `mscent-render`, `mscent-gpu` or
materials also gets the `render-parity-reviewer` agent's review.

## Benchmarks

`cargo run --release -p mscent-bench --example bump_bench`
