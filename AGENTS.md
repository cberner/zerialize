## Setup

`just` is required.
Several of the justfile targets need rootless Podman. Their Rust toolchain and Linux build
dependencies are installed in the sandbox image automatically. Other recipes
run on the host and may require their corresponding Rust tools, targets, and
system packages locally.

The CI workflow pins specific versions; prefer those versions if you hit incompatibilities. See
`.github/workflows/ci.yml` for the exact list CI uses.

## Before completing a task

**Always run `just test` and confirm it passes before telling the user you are done.**
This target runs `cargo deny check licenses` during network-enabled dependency
preparation, then disables networking and runs `cargo fmt --check`,
`cargo clippy --all-targets --all-features`, and `cargo test --all-features` with
`RUST_BACKTRACE=1`. If any of those fail, fix the underlying issue — do not bypass
checks.

## Style guide
- Comments should be brief and focus on important invariants, architectural details, or other
  long-term relevant information. They should not contain minor implementation details of the current
  commit.

## Git commits
Make one commit per feature / bug fix when opening a PR. Multiple commits or "fixup" commits are
should not be merged to master.

## Other notes

- The repo enforces ASCII-only source: CI fails on non-ASCII characters in
  `*.rs` and `*.toml` files. Keep new code ASCII-only.
