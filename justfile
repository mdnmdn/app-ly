# app-ly — generic Tauri shell

default:
    @just --list

# Install npm dependencies
install:
    npm install

# Run the shell in dev mode (uses ./app.toml)
dev:
    npm run tauri dev

# Run dev with a custom config file
dev-config config:
    npm run tauri dev -- --config {{config}}

# Build release bundle
build:
    npm run tauri build

# Compile Rust only (debug)
cargo-build:
    cargo build --manifest-path src-tauri/Cargo.toml

# Compile Rust only (release)
cargo-build-release:
    cargo build --manifest-path src-tauri/Cargo.toml --release

# Check Rust without producing binaries
check:
    cargo check --manifest-path src-tauri/Cargo.toml

# Format Rust code
fmt:
    cargo fmt --manifest-path src-tauri/Cargo.toml --all

# Remove Rust and Tauri build artifacts
clean:
    cargo clean --manifest-path src-tauri/Cargo.toml
    rm -rf src-tauri/target