#!/bin/bash
# Second node per chain (Miningcore requires a peer); loopback only. The Sedra peer also syncs the merged-mined
# blocks from the first Sedra node over the real P2P protocol.
COMMON="--devnet --utxoindex --nodnsseed --disable-upnp --enable-unsynced-mining --yes"
pkill -f "e2e-kas2" ; pkill -f "e2e-sdr2"; sleep 1
setsid nohup ~/e2e-kaspad $COMMON --appdir=$HOME/e2e-kas2 --rpclisten=127.0.0.1:16620 --listen=127.0.0.1:16621 --connect=127.0.0.1:16611 \
  > ~/e2e-kaspad2.log 2>&1 < /dev/null &
setsid nohup ~/e2e-sedrad $COMMON --devnet-aux-pow-activation=0 --appdir=$HOME/e2e-sdr2 --rpclisten=127.0.0.1:22620 --listen=127.0.0.1:22621 --connect=127.0.0.1:22611 \
  > ~/e2e-sedrad2.log 2>&1 < /dev/null &
sleep 8
echo "--- peer logs"; grep -iE "error|panic|peer|connect" ~/e2e-kaspad2.log ~/e2e-sedrad2.log | cut -c1-200 | head -8
ss -ltn | grep -E "16620|16621|22620|22621"
