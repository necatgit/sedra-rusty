#!/bin/bash
# Local, isolated devnet nodes for the end-to-end test: loopback only, no DNS seeding, no peers.
S=$(cd "$(dirname "$0")" && pwd)
source ~/.cargo/env

# the miner (once)
if [ ! -x ~/e2e-miner-bin ]; then
  rm -rf ~/e2e-miner && cp -r $S/miner ~/e2e-miner && sed -i 's/\r$//' ~/e2e-miner/Cargo.toml ~/e2e-miner/src/main.rs
  (cd ~/e2e-miner && CARGO_TARGET_DIR=~/e2e-miner/target cargo build --release 2>&1 | grep -E "^error|warning: unused|Finished" | head -5)
  cp ~/e2e-miner/target/release/e2e-miner ~/e2e-miner-bin
fi

pkill -f "e2e-kaspad" ; pkill -f "e2e-sedrad" ; sleep 1
COMMON="--devnet --utxoindex --nodnsseed --disable-upnp --enable-unsynced-mining --yes"
setsid nohup ~/e2e-kaspad $COMMON --appdir=$HOME/e2e-kas --rpclisten=127.0.0.1:16610 --listen=127.0.0.1:16611 \
  > ~/e2e-kaspad.log 2>&1 < /dev/null &
setsid nohup ~/e2e-sedrad $COMMON --devnet-aux-pow-activation=0 --appdir=$HOME/e2e-sdr --rpclisten=127.0.0.1:22610 --listen=127.0.0.1:22611 \
  > ~/e2e-sedrad.log 2>&1 < /dev/null &
sleep 8
echo "--- kaspa devnet"; grep -iE "error|panic|listening|network|version" ~/e2e-kaspad.log | head -8
echo "--- sedra devnet"; grep -iE "error|panic|listening|network|version|aux" ~/e2e-sedrad.log | head -8
ss -ltn | grep -E "16610|16611|22610|22611"

# second node per chain (Miningcore requires a peer); the Sedra peer also syncs the merged-mined blocks over P2P
pkill -f "e2e-kas2" ; pkill -f "e2e-sdr2"; sleep 1
setsid nohup ~/e2e-kaspad $COMMON --appdir=$HOME/e2e-kas2 --rpclisten=127.0.0.1:16620 --listen=127.0.0.1:16621 --connect=127.0.0.1:16611 \
  > ~/e2e-kaspad2.log 2>&1 < /dev/null &
setsid nohup ~/e2e-sedrad $COMMON --devnet-aux-pow-activation=0 --appdir=$HOME/e2e-sdr2 --rpclisten=127.0.0.1:22620 --listen=127.0.0.1:22621 --connect=127.0.0.1:22611 \
  > ~/e2e-sedrad2.log 2>&1 < /dev/null &
sleep 6
echo "--- peers"; grep -iE "error|panic|peer|connect" ~/e2e-kaspad2.log ~/e2e-sedrad2.log | head -8
ss -ltn | grep -E "16620|16621|22620|22621"
