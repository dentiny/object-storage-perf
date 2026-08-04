#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rustup_bin="${RUSTUP_BIN:-$(command -v rustup || true)}"
toolchain="${RUST_TOOLCHAIN:-1.91.1}"
target="x86_64-unknown-linux-musl"
repository="${IMAGE_REPOSITORY:-phx.ocir.io/axnzj5nsewcd/object-storage-perf}"
tag="${IMAGE_TAG:-local-$(date -u +%Y%m%d%H%M%S)}"
image="${repository}:${tag}"
context="$(mktemp -d)"
trap 'rm -rf "$context"' EXIT
zig_ar="$context/x86_64-unknown-linux-musl-ar"

# Host ar cannot archive Linux objects when cross-compiling on macOS.
printf '%s\n' '#!/bin/sh' 'exec zig ar "$@"' >"$zig_ar"
chmod +x "$zig_ar"

if [[ -z "$rustup_bin" || ! -x "$rustup_bin" ]]; then
  echo "rustup is required; install it with: brew install rustup" >&2
  exit 1
fi
if ! command -v zig >/dev/null; then
  echo "Zig is required; install it with: brew install zig" >&2
  exit 1
fi
if ! PATH="$HOME/.cargo/bin:$PATH" cargo zigbuild --help >/dev/null; then
  echo "cargo-zigbuild is required; install it with: cargo install cargo-zigbuild" >&2
  exit 1
fi
if ! docker buildx version >/dev/null; then
  echo "Docker Buildx is required; install and enable the docker-buildx plugin." >&2
  exit 1
fi

cd "$root"
rustc_bin="$("$rustup_bin" which rustc --toolchain "$toolchain")"
CARGO_TARGET_DIR="$root/target" \
  PATH="$context:$HOME/.cargo/bin:$PATH" \
  RUSTC="$rustc_bin" \
  "$rustup_bin" run "$toolchain" \
  cargo zigbuild --target "$target" --release --locked

cp "$root/Dockerfile" "$context/Dockerfile"
cp "$root/target/$target/release/object-storage-perf" "$context/object-storage-perf"

if [[ "${PUSH_IMAGE:-1}" == "1" ]]; then
  docker buildx build \
    --platform linux/amd64 \
    --tag "$image" \
    --push \
    "$context"
else
  docker buildx build \
    --platform linux/amd64 \
    --tag "$image" \
    --load \
    "$context"
fi

printf 'BENCHMARK_IMAGE=%s\n' "$image"
