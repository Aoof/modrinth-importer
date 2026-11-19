# Contributing

## Branching
- `master`: released code. Every push to `master` publishes a release.
- `develop`: integration branch and the default branch. Open PRs against it.
- `feat/*`, `fix/*`, `docs/*`, `ci/*`, `chore/*`: short-lived topic branches off `develop`.
- Releases: open a PR from `develop` to `master` and **merge it with a merge commit** (not squash) so the release workflow can read the individual Conventional Commits.

## Commits
[Conventional Commits](https://www.conventionalcommits.org): `feat:` bumps the minor version, `fix:`/anything else the patch, and `feat!:` / `BREAKING CHANGE:` is a breaking change (minor while we're on 0.x, major after 1.0).

## Local development
```sh
pnpm --dir ui install
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo tauri dev        # needs: cargo install tauri-cli --locked
```
Linux also needs Tauri's system libraries; see the `rust` job in `.github/workflows/ci.yml`.

## Adding a source
Add a function in `crates/core/src/providers/`, teach `classify` in `providers/mod.rs` to recognise it, and add a fixture-based test in `crates/core/tests/`.
