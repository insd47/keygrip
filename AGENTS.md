# Agent Instructions

## Versioning: Git tags only

- Versions are managed exclusively through Git tags (`vX.Y.Z`). The in-repo
  versions are placeholders, both in the root `Cargo.toml`:
  `workspace.package.version = "0.0.0"` and the `keygrip-derive` entry's
  `=0.0.0` pin in `[workspace.dependencies]`. **Never bump versions in
  Cargo.toml files** — `.github/workflows/publish.yml` injects the tag version
  into both lines at publish time.
- Publishing = the owner pushes a `vX.Y.Z` tag. CI then tests the workspace and
  publishes `keygrip-derive` first, `keygrip` second (the exact `=` pin makes
  this order mandatory). Both crates always release lockstep, even when one has
  no changes.
- Agents never publish. After changing crate code, stop and let the owner tag.

## Conventions

- This is a public crates.io crate: all rustdoc, README, and code comments are
  written in **English**, in the existing house style (see `occ.rs` /
  `request.rs` module docs). Commit messages are Korean.
- Keep the API surface minimal and semver-deliberate — additions are debt.
  Domain/app-specific operations belong in consumer crates, not here.
- Verification: `cargo fmt --all --check && cargo clippy --all-targets &&
  cargo test` (doctests included).
