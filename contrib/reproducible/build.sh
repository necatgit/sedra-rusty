#!/usr/bin/env bash
# SEDRA: reproducible release build of the Sedra node (the `kaspad` binary) for linux-amd64.
#
# Same commit + same build environment = byte-identical binary and archive. The supported environment is the
# pinned container in contrib/reproducible/Dockerfile (run contrib/reproducible/build-in-docker.sh); running this script
# directly works too, but then the result depends on the host's C/C++ compilers.
#
# Output in dist/: sedra-rusty-<release>-linux-amd64.tar.gz and SHA256SUMS (archive and binary).
# See docs/RELEASE.md for how to verify a published release.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
ROOT=$PWD

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    echo "build.sh: the working tree has uncommitted changes; a release is built from a commit" >&2
    exit 1
fi

RELEASE=$(sed -n 's/^    "\([0-9][0-9.]*[^"]*\)"$/\1/p' core/src/kaspad_env.rs | head -n 1)
[ -n "$RELEASE" ] || { echo "build.sh: release not found in core/src/kaspad_env.rs" >&2; exit 1; }
COMMIT=$(git rev-parse HEAD)
NAME="sedra-rusty-${RELEASE}-linux-amd64"
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}

# Everything that could differ between two builders is fixed here.
export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="$ROOT/target/release-build"
export TZ=UTC LC_ALL=C
# the commit embedded by utils/build.rs: `git rev-parse --short` length depends on the clone, so fix it at 12
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.abbrev GIT_CONFIG_VALUE_0=12
# build paths must not end up in the binary
MAP="--remap-path-prefix=$ROOT=/sedra-rusty --remap-path-prefix=$CARGO_HOME=/cargo"
export RUSTFLAGS="$MAP"
CMAP="-ffile-prefix-map=$ROOT=/sedra-rusty -ffile-prefix-map=$CARGO_HOME=/cargo"
export CFLAGS="$CMAP"
# keep the RocksDB fix from .cargo/config.toml (setting CXXFLAGS here replaces it)
export CXXFLAGS="-include cstdint $CMAP"

echo "Building $NAME from $COMMIT (rustc $(rustc --version | cut -d' ' -f2))"
cargo build --release --locked -p kaspad

STAGE="$ROOT/dist/$NAME"
rm -rf "$STAGE" && mkdir -p "$STAGE"
cp "$CARGO_TARGET_DIR/release/kaspad" "$STAGE/kaspad"
cp LICENSE SEDRA.md "$STAGE/"
cp docs/RELEASE.md "$STAGE/RELEASE.md"
printf '%s\n' "sedra-rusty $RELEASE" "commit $COMMIT" "rustc $(rustc --version)" > "$STAGE/BUILDINFO"

# a deterministic archive: sorted names, fixed times and owners, no gzip timestamp
find "$STAGE" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner --format=gnu \
    -C "$ROOT/dist" -cf - "$NAME" | gzip -n -9 > "$ROOT/dist/$NAME.tar.gz"

( cd "$ROOT/dist" && sha256sum "$NAME.tar.gz" "$NAME/kaspad" > SHA256SUMS && cat SHA256SUMS )
# SEDRA: kaspad exits 1 after printing the version (upstream clap handling), which made this script fail
"$STAGE/kaspad" --version || true
