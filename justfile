_default:
    @just --list

# Check all targets for default and each single-backend feature set
check:
    cargo check --all-targets
    cargo check --all-targets --no-default-features -F json
    cargo check --all-targets --no-default-features -F sqlite
    cargo check --all-targets --no-default-features -F vjournal
    cargo check --all-targets --no-default-features -F file

# Run all tests
test:
    cargo test

# Lint with the same flags as CI
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Verify the dependency/license snapshot and distributable notices without network access
licenses:
    python3 scripts/generate-notices.py --check

# Install mjournal; extra args go to cargo install, e.g. `just install --no-default-features -F sqlite`
install *args:
    cargo install --path . --locked {{ args }}
