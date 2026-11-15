#!/bin/bash
# Go wallet daemon sedrawallet (sedrad v2.0.0) against the Rust node (sedra-rusty) on a local devnet.
# Checks what pools, the game and exchanges rely on: addresses, balance, a CLI send (unsigned tx + local signing +
# broadcast), and the daemon's own Send as Miningcore's payout calls it. Merged mining stays off (no activation flag).
# Needs ~/e2e-sedrad and ~/e2e-boot-sedra-bin (build_nodes.sh, build_boot.sh) and ~/repos/sedrad at v2.0.0.
set -u
S=$(cd "$(dirname "$0")" && pwd)
W=~/e2e-sedrawallet
KF=$HOME/e2e-wallet/keys.json
PW=e2e-test-only
D=127.0.0.1:8099
NODE=127.0.0.1:22610

if [ ! -x $W ] || [ ! -x ~/e2e-walletcheck ]; then
  rm -rf ~/e2e-sedrad-src && cp -r ~/repos/sedrad ~/e2e-sedrad-src && rm -rf ~/e2e-sedrad-src/.git
  mkdir -p ~/e2e-sedrad-src/cmd/walletcheck && cp $S/walletcheck/main.go ~/e2e-sedrad-src/cmd/walletcheck/
  (cd ~/e2e-sedrad-src && go build -o $W ./cmd/sedrawallet && go build -o ~/e2e-walletcheck ./cmd/walletcheck) || exit 1
fi

pkill -x e2e-sedrad; pkill -x e2e-sedrawallet; sleep 1
rm -rf ~/e2e-sdr ~/e2e-wallet && mkdir -p ~/e2e-wallet
setsid nohup ~/e2e-sedrad --devnet --utxoindex --nodnsseed --disable-upnp --enable-unsynced-mining --yes \
  --appdir=$HOME/e2e-sdr --rpclisten=$NODE --listen=127.0.0.1:22611 > ~/e2e-sedrad.log 2>&1 < /dev/null &
sleep 8

$W create --devnet -f $KF -p $PW -y > ~/e2e-wallet/create.log 2>&1 || { echo "create FAILED"; cat ~/e2e-wallet/create.log; exit 1; }
setsid nohup $W start-daemon --devnet -f $KF -p $PW -s $NODE -l $D > ~/e2e-wallet/daemon.log 2>&1 < /dev/null &
sleep 5

A1=$($W new-address --devnet -d $D 2>&1 | grep -o 'sedradev:[a-z0-9]*' | tail -1)
A2=$($W new-address --devnet -d $D 2>&1 | grep -o 'sedradev:[a-z0-9]*' | tail -1)
OTHER=$(python3 $S/addr.py sedradev 77 | tail -1)
echo "wallet addresses: $A1 $A2; outside address: $OTHER"
[ -n "$A1" ] && [ -n "$A2" ] || { echo "new-address FAILED"; tail -5 ~/e2e-wallet/daemon.log; exit 1; }

echo "--- mine 130 blocks to the wallet (coinbase maturity), then 20 to an outside address"
~/e2e-boot-sedra-bin grpc://$NODE $A1 130 2>&1 | tail -1
~/e2e-boot-sedra-bin grpc://$NODE $OTHER 20 2>&1 | tail -1
sleep 3
echo "--- balance (CLI)"; $W balance --devnet -d $D -v 2>&1 | tail -4
echo "--- daemon GetBalance (Miningcore's call)"; ~/e2e-walletcheck $D $PW $OTHER 0

echo "--- CLI send 10 SDR to the wallet's second address"
$W send --devnet -d $D -f $KF -p $PW -t $A2 -v 10 2>&1 | tail -3
echo "--- daemon Send 7 SDR to the outside address (Miningcore's payout call)"
~/e2e-walletcheck $D $PW $OTHER 7

echo "--- mine 5 blocks so both transactions are included"
~/e2e-boot-sedra-bin grpc://$NODE $OTHER 5 2>&1 | tail -1
sleep 3
echo "--- balance afterwards (CLI, per address)"; $W balance --devnet -d $D -v 2>&1 | tail -6
echo "--- daemon GetBalance afterwards (expected: 7 SDR plus fees less; the 10 SDR stay in the wallet)"
~/e2e-walletcheck $D $PW $OTHER 0
echo "--- errors in the node and daemon logs"
grep -iE "error|panic|reject" ~/e2e-sedrad.log | grep -v -i "peer" | head -5
grep -iE "error|panic" ~/e2e-wallet/daemon.log | head -5
echo "done"
