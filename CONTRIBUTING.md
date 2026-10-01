# Contributing

keygrip is a Cargo workspace of two crates released in lockstep: `keygrip` and its proc-macro `keygrip-derive`.
`main` is protected; every change lands through a pull request.

## Pull requests

- Open a pull request against `main`. It merges once the `check` and `title` checks pass and the branch is up to date
  with `main`.
- Pull requests are squash-merged. Keep the suggested commit title when merging (`<PR title> (#N)`); it becomes the
  commit on `main` and the changelog line.
- Commits inside a branch are free-form. Only the pull request title is kept.

### Stacked pull requests

Split a change that has several reviewable steps into a
[stack](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs): each pull request targets the branch
of the one below it, and the bottom one targets `main`. Create the stack from "Preview stack" once every pull request
is open.

- Every pull request in the stack runs `check` and `title`, and each one still lands as its own squash commit and
  changelog line.
- Merge the top pull request once all checks pass; everything below it lands in order. Merging a lower pull request on
  its own restacks the rest and reruns their checks.
- A change to the workflows belongs at the bottom of the stack. `title` runs on `pull_request_target`, which reads its
  trigger from the workflow file on `main`, so upper pull requests pick up the change only after it lands.

## Pull request titles

Titles follow [Conventional Commits](https://www.conventionalcommits.org/) with an English subject:
`type: subject` or `type(scope): subject`. The `title` check enforces the format.

The type decides where the change appears in the changelog and whether it starts a release:

| Type                            | Changelog | Starts a release |
|---------------------------------|-----------|------------------|
| any type with `!` (`feat!:`)    | Changed   | yes              |
| `feat`                          | Added     | yes              |
| `fix`                           | Fixed     | yes              |
| `perf`, `revert`                | Changed   | yes              |
| `docs`, `refactor`, `test`, ... | omitted   | no               |

## Breaking changes

Mark a breaking change with `!` in the title. A change is breaking when existing code stops compiling or behaving the
same, and also when the stored item format changes (key encoding, attribute names), since existing tables can no
longer be read as before.

> The semver check in the release pull request only sees the public API. It cannot detect a storage format change, so
> the `!` is the only signal for those.

## Releases

Releases are automated with [release-plz](https://release-plz.dev).

1. When a release-starting pull request merges, release-plz opens or updates a release pull request
   (`chore: release vX.Y.Z`) that bumps both crates and adds a section to `CHANGELOG.md`.
2. Before merging it, edit that changelog section if users need more than the generated lines, such as migration
   notes for a breaking change. Edit right before merging, because release-plz rewrites the pull request whenever
   `main` changes.
3. Merging the release pull request publishes `keygrip-derive` and `keygrip` to crates.io, tags `vX.Y.Z`, and creates
   a GitHub release from the changelog section.

Never bump versions or edit released changelog sections by hand. The release pull request is opened with the
`RELEASE_PLZ_TOKEN` secret, a fine-grained personal access token limited to this repository (Contents and Pull
requests: read and write). Renew it before it expires; an expired token makes the release pull request job fail.

## Local checks

The `check` workflow runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Style

- Rustdoc, README, comments, workflow text, and pull request titles are written in English.
- Keep the public API small. Domain-specific operations belong in the consuming crates, not here.
