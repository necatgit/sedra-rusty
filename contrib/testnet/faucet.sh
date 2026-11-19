#!/bin/bash
# SEDRA (#13): testnet faucet. Sends a fixed amount of testnet SDR to one sedratest: address from the operator's
# faucet wallet (Go sedrawallet v2.0.0, daemon running with --testnet). Requests arrive as GitHub issues; the operator
# runs this by hand and answers with the transaction ID. Testnet coins have no value.
#
# Usage: FAUCET_PASSWORD=... bash faucet.sh sedratest:ADDRESS [AMOUNT]
#   FAUCET_KEYS     keys file of the faucet wallet (default ~/.sedrawallet/sedra-testnet-10/keys.json)
#   FAUCET_DAEMON   wallet daemon address (default localhost:8082)
#   SEDRAWALLET     the sedrawallet binary (default: sedrawallet on PATH)
set -euo pipefail
TO=${1:?usage: faucet.sh sedratest:ADDRESS [AMOUNT]}
AMOUNT=${2:-1000}
: "${FAUCET_PASSWORD:?set FAUCET_PASSWORD (the faucet wallet password; never a mainnet wallet)}"
KEYS=${FAUCET_KEYS:-$HOME/.sedrawallet/sedra-testnet-10/keys.json}
DAEMON=${FAUCET_DAEMON:-localhost:8082}
W=${SEDRAWALLET:-sedrawallet}

case "$TO" in
  sedratest:*) ;;
  *) echo "refusing '$TO': only sedratest: addresses" >&2; exit 2 ;;
esac
# a modest cap, so a typo cannot drain the faucet
if ! awk -v a="$AMOUNT" 'BEGIN { exit !(a > 0 && a <= 100000) }'; then
  echo "amount must be > 0 and <= 100000 testnet SDR" >&2; exit 2
fi

"$W" send --testnet -f "$KEYS" -d "$DAEMON" -p "$FAUCET_PASSWORD" -t "$TO" -v "$AMOUNT" -u
echo "$(date -u +%FT%TZ) sent $AMOUNT to $TO" >> "${FAUCET_LOG:-$HOME/faucet.log}"
