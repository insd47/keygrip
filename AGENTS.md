# Agent Instructions

## Versioning: release-plz

- The version lives in the root `Cargo.toml`, in two lines that always move
  together: `workspace.package.version` and the `keygrip-derive` entry's `=`
  pin in `[workspace.dependencies]`. Both crates release lockstep.
- Publishing = a push to `main` that touches the crates.
  `.github/workflows/publish.yml` runs the checks and `release-plz release`,
  which publishes only versions missing from crates.io (`keygrip-derive`
  first). `release-plz.toml` gives each version one `vX.Y.Z` tag and GitHub
  release, owned by `keygrip`.
- Agents never publish or push, and bump the version only when the owner asks.

## Conventions

- This is a public crates.io crate: all rustdoc, README, and code comments are
  written in **English**, in the existing house style (see `occ.rs` /
  `request.rs` module docs). Commit messages are Korean.
- Keep the API surface minimal and semver-deliberate — additions are debt.
  Domain/app-specific operations belong in consumer crates, not here.
- Verification: `cargo fmt --all --check && cargo clippy --all-targets &&
  cargo test` (doctests included).
