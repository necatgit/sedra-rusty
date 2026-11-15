#!/bin/bash
# Builds the two nodes for the local end-to-end test (M10 step 5c).
set -e
source ~/.cargo/env
cd ~/repos/sedra-rusty
git checkout -q m10-auxpow
cargo build --release --bin kaspad 2>&1 | tail -2
cp target/release/kaspad ~/e2e-sedrad
echo "sedra node: $(~/e2e-sedrad --version)"
if [ ! -d ~/repos/rusty-kaspa-e2e ]; then
  git clone -q /mnt/c/Users/Nec/repos/miningcore/upstream/rusty-kaspa ~/repos/rusty-kaspa-e2e
fi
cd ~/repos/rusty-kaspa-e2e
cargo build --release --bin kaspad 2>&1 | tail -2
cp target/release/kaspad ~/e2e-kaspad
echo "kaspa node: $(~/e2e-kaspad --version)"
