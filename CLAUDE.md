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

## Releases
- `.github/workflows/release.yml` publishes a release when a `v*` tag is pushed: the same checks,
  then `FlexTaskbar.exe`, `SHA256SUMS.txt` and a zip, with the commits since the last tag as notes.
- The tag must match `version` in `Cargo.toml` (the workflow fails otherwise): the app compares a
  release's tag with the version it was built with, so a mismatch would offer the same update for ever.
- Never publish the program without `SHA256SUMS.txt` beside it — the app refuses to install a release
  it can't check, and only offers its page instead.

## Two forges
- `origin` (git.zacx.net/owner/flex-taskbar, Forgejo) and the public mirror github.com/mzaki-raup/flex-taskbar
  share one history. `origin` has both as push URLs, so `git push origin <branch|tag>` goes to both; the
  `github` remote is for fetching what was committed there. Bring GitHub commits back with
  `git fetch github && git merge --ff-only github/main && git push origin main` — never force-push either side.
- Versions and tags are made on Forgejo only. A tag runs `release.yml` on both forges; each publishes its
  own release. The GitHub build sets `FLEX_RELEASES_API` (read by `src/win/update.rs`) so that copy polls
  GitHub for updates; unset means git.zacx.net. Keep both publish steps in `release.yml` in step.
- Both forges run `build.yml` on pushed branches, so a branch's CI must be green on both before merging.
