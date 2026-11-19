#!/bin/bash
# SEDRA (#13): builds the public-testnet tools (sedra-testnet-miner, sedra-testnet-monitor) against this checkout's
# crates, starting from the node's own Cargo.lock (only the tools package is added to it), into contrib/testnet/tools/target/release/.
set -euo pipefail
S=$(cd "$(dirname "$0")" && pwd)
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
cp "$S/../../Cargo.lock" "$S/tools/Cargo.lock"
cd "$S/tools"
cargo build --release 2>&1 | grep -E "^(error|warning: unused)|-->|Finished" || true
ls -l target/release/sedra-testnet-miner target/release/sedra-testnet-monitor
