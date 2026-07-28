build: pre
    ./scripts/podman-sandbox.sh 'cargo build --frozen --all-targets --all-features'
    ./scripts/podman-sandbox.sh 'cargo doc --frozen'

pre: _audit _checks

pre_no_sandbox: _audit _checks_no_sandbox

_audit:
    cargo deny --all-features check licenses advisories
    cargo fmt --all -- --check

_checks:
    ./scripts/podman-sandbox.sh 'just _checks_no_sandbox'

_checks_no_sandbox:
    cargo clippy --all-targets --all-features

test: pre
    ./scripts/podman-sandbox.sh 'RUST_BACKTRACE=1 cargo test --frozen --all-features'

test_no_sandbox: pre_no_sandbox
    RUST_BACKTRACE=1 cargo test --frozen --all-features

clear_podman_cache:
    podman volume rm --force zerialize-sandbox-target

coverage:
    #!/usr/bin/env bash
    set -euxo pipefail
    cargo install --locked cargo-llvm-cov
    rustup component add llvm-tools-preview
    RUST_BACKTRACE=1 cargo llvm-cov --all-features

format:
    cargo fmt --all
