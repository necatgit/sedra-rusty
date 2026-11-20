#!/bin/bash
# SEDRA (#13): private dry run of the public-testnet kit on one test host. Everything listens on loopback only, with
# its own data directory ($DRY) and ports; nothing on the mainnet ports (221xx) is touched, no DNS seeding, no outside
# peers. Every process runs in its own user scope with a MemoryMax.
#
# Runs: two upstream rusty-kaspa nodes on a private, loopback-only Kaspa testnet-10 chain from genesis as the parent (a
# stand-in for Kaspa's public testnet-10: same network type, so the pool's address checks see testnet on both sides;
# a Kaspa devnet parent makes Miningcore refuse sedratest: addresses, found in this dry run 2026-11-20), two Sedra
# testnet nodes from this branch with --aux-pow-activation-override=$ACT (A mines, B follows over P2P like a
# participant), an unmodified Go sedrad v2.0.0 on the testnet (the old node, criterion 3), the monitor, a native CPU
# miner and the Go sedrawallet daemon. The pool (Miningcore) and the stratum miner run elsewhere through SSH tunnels.
#
# Keep the SSH session that runs this open for the whole dry run: without lingering, the user scopes end with it.
# Usage: ACT=4000 bash start.sh ; bash stop.sh [--wipe]
set -euo pipefail
DRY=${DRY:-/data/testnet-dryrun}
ACT=${ACT:-4000}
BIN=$DRY/bin
WALLET_PW=${WALLET_PW:-dryrun-only}
mkdir -p "$DRY/logs"
cd "$DRY"

run() { # name memory command...
  local name=$1 mem=$2; shift 2
  systemd-run --user --scope -q --unit="sedra-dryrun-$name" -p MemoryMax="$mem" "$@" > "$DRY/logs/$name.log" 2>&1 &
  echo "started $name (MemoryMax $mem)"
}

# private Kaspa testnet-10 chain: no DNS seeding, loopback only, kas2 --connect to kas1, so it never meets the public
# Kaspa testnet-10 (with --connect a node accepts no inbound peers, so kas1 must not use it)
COMMON_KAS="--testnet --netsuffix=10 --utxoindex --nodnsseed --disable-upnp --enable-unsynced-mining --yes"
run kas1 2G "$BIN/kaspad-kaspa" $COMMON_KAS --appdir="$DRY/kastn1" --rpclisten=127.0.0.1:16210 --listen=127.0.0.1:16211
run kas2 1G "$BIN/kaspad-kaspa" $COMMON_KAS --appdir="$DRY/kastn2" --rpclisten=127.0.0.1:16220 --listen=127.0.0.1:16221   --connect=127.0.0.1:16211

# Sedra testnet node A: the "seed" (unsynced mining only to start the fresh chain)
run sdrA 2G "$BIN/kaspad" --testnet --aux-pow-activation-override="$ACT" --utxoindex --nodnsseed --disable-upnp \
  --enable-unsynced-mining --yes --appdir="$DRY/sdrA" --rpclisten=127.0.0.1:22210 --listen=127.0.0.1:22211
sleep 5
# node B: started as the participant guide says, but --connect to A instead of --addpeer to public seeds
run sdrB 2G "$BIN/kaspad" --testnet --aux-pow-activation-override="$ACT" --utxoindex --nodnsseed --disable-upnp \
  --yes --appdir="$DRY/sdrB" --rpclisten=127.0.0.1:22220 --listen=127.0.0.1:22221 --connect=127.0.0.1:22211
# the old node: unmodified Go sedrad v2.0.0
run goold 2G "$BIN/sedrad" --testnet --appdir="$DRY/goold" --nodnsseed --listen=127.0.0.1:22231 \
  --rpclisten=127.0.0.1:22230 --connect=127.0.0.1:22211
sleep 8

# private Kaspa testnet: a few blocks so the parent node reports synced
"$BIN/e2e-boot-kaspa" grpc://127.0.0.1:16210 kaspatest:qq3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zy09kj3t9w 3 | tail -1

# wallet (Go sedrawallet v2.0.0): create once, daemon against node A
if [ ! -f "$DRY/wallet/keys.json" ]; then
  mkdir -p "$DRY/wallet"
  "$BIN/sedrawallet" create --testnet -f "$DRY/wallet/keys.json" -p "$WALLET_PW" -y > "$DRY/logs/wallet-create.log" 2>&1
fi
run walletd 512M "$BIN/sedrawallet" start-daemon --testnet -f "$DRY/wallet/keys.json" -p "$WALLET_PW" \
  --rpcserver=127.0.0.1:22210 --listen=127.0.0.1:8182
sleep 4
ADDR=$("$BIN/sedrawallet" new-address --testnet -d 127.0.0.1:8182 | grep -o 'sedratest:[a-z0-9]*' | head -1)
echo "$ADDR" > "$DRY/miner-address"
echo "miner/wallet address: $ADDR"

run miner 512M "$BIN/sedra-testnet-miner" --rpc grpc://127.0.0.1:22210 --address "$ADDR" --threads 2 --extra-data dryrun-native
SEDRACTL="$BIN/sedractl" run monitor 512M "$BIN/sedra-testnet-monitor" --out "$DRY/monitor" --node A=grpc://127.0.0.1:22210 \
  --node B=grpc://127.0.0.1:22220 --node old-go=sedractl://127.0.0.1:22230 --interval 20 --depth 30
echo "activation DAA $ACT; logs in $DRY/logs; monitor CSVs in $DRY/monitor"
