# Working on FlexTaskbar

## Workflow
- After a change is tested (local `cargo test` + `cargo clippy --target x86_64-pc-windows-gnu -- -D warnings`,
  and the Windows CI run on the pushed branch is green) and the documentation (README.md) is updated
  to match, merge the branch into `main` and push `main`. The repository owner asked for this to be
  done every time without asking again.
- Never merge while CI is red.

## Checks
- `cargo fmt --check`
- `cargo test` (platform-independent logic; runs on Linux too)
- `cargo clippy --release --target x86_64-pc-windows-gnu -- -D warnings` (needs `mingw-w64` on Linux)
- CI (`.github/workflows/build.yml`) runs the same on `windows-latest` with the latest stable Rust,
  which can have newer clippy lints than a local toolchain.
