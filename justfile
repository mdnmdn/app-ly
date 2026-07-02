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

# Bump version, commit, tag (e.g. `just publish-version v0.26.0`), and push
publish-version version:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{version}}"
    version="${version#v}"
    tag="v$version"

    if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
        echo "error: tag $tag already exists locally" >&2
        exit 1
    fi
    if git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
        echo "error: tag $tag already exists on origin" >&2
        exit 1
    fi
    if ! git diff --cached --quiet; then
        echo "error: git stage is not empty, commit or unstage before publishing" >&2
        exit 1
    fi

    tmp=$(mktemp)
    jq --arg v "$version" '.version = $v' package.json > "$tmp" && mv "$tmp" package.json

    tmp=$(mktemp)
    jq --arg v "$version" '.version = $v' src-tauri/tauri.conf.json > "$tmp" && mv "$tmp" src-tauri/tauri.conf.json

    sed -i.bak "s/^version = \".*\"/version = \"$version\"/" src-tauri/Cargo.toml
    rm -f src-tauri/Cargo.toml.bak

    git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    git commit -m "release: $tag"
    git tag "$tag"
    git push
    git push origin "$tag"