#!/usr/bin/env bash
# SEDRA: run the reproducible release build (build.sh) inside the pinned container (Dockerfile).
# Needs Docker or Podman. The checkout is mounted at /sedra-rusty, the cargo download cache lives in target/cargo-home.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
ENGINE=${ENGINE:-$(command -v docker || command -v podman || true)}
[ -n "$ENGINE" ] || { echo "build-in-docker.sh: Docker or Podman is required" >&2; exit 1; }

IMAGE=sedra-rusty-release:$(sha256sum contrib/reproducible/Dockerfile | cut -c1-12)
"$ENGINE" build -t "$IMAGE" -f contrib/reproducible/Dockerfile contrib/reproducible
# Rootless Podman maps user IDs: keep-id makes the build user own the mounted checkout, as -u does with Docker.
USERNS=()
case "$(basename "$ENGINE")" in podman*) USERNS=(--userns=keep-id) ;; esac
"$ENGINE" run --rm "${USERNS[@]}" \
    -v "$PWD:/sedra-rusty" \
    -u "$(id -u):$(id -g)" -e HOME=/tmp -e CARGO_HOME=/sedra-rusty/target/cargo-home \
    "$IMAGE" bash contrib/reproducible/build.sh
