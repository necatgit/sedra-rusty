#!/bin/bash
# Builds e2e-boot twice: against sedra-rusty (Sedra node) and against upstream rusty-kaspa (Kaspa node).
set -e
source ~/.cargo/env
S=$(cd "$(dirname "$0")" && pwd)
for flavor in sedra kaspa; do
  if [ $flavor = sedra ]; then R=$HOME/repos/sedra-rusty; else R=$HOME/repos/rusty-kaspa-e2e; fi
  D=$HOME/e2e-boot-$flavor
  rm -rf $D && mkdir -p $D/src
  sed 's/\r$//' $S/boot/src/main.rs > $D/src/main.rs
  cat > $D/Cargo.toml <<TOML
[package]
name = "e2e-boot"
version = "0.1.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
kaspa-addresses = { path = "$R/crypto/addresses" }
kaspa-consensus-core = { path = "$R/consensus/core" }
kaspa-grpc-client = { path = "$R/rpc/grpc/client" }
kaspa-rpc-core = { path = "$R/rpc/core" }
kaspa-pow = { path = "$R/consensus/pow" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
TOML
  cp $R/Cargo.lock $D/
  (cd $D && CARGO_TARGET_DIR=$D/target cargo build --release 2>&1 | grep -E "^error|-->|Finished" | head -8)
  cp $D/target/release/e2e-boot $HOME/e2e-boot-$flavor-bin
  echo "built $flavor"
done
