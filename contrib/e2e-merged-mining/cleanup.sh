#!/bin/bash
# Stops the end-to-end test processes and deletes the throwaway data (binaries and scripts stay for a rerun).
pkill -x e2e-kaspad
pkill -x e2e-sedrad
pkill -x e2e-miner-bin
/usr/lib/postgresql/16/bin/pg_ctl -D ~/e2e-pgdata stop -m fast 2>&1 | tail -1
sleep 2
pgrep -x e2e-kaspad; pgrep -x e2e-sedrad; pgrep -f "postgres -D /home/nec/e2e-pgdata"
rm -rf ~/e2e-pgdata ~/e2e-kas ~/e2e-kas2 ~/e2e-sdr ~/e2e-sdr2 ~/e2e-pg
echo "left in ~: $(ls -d ~/e2e-* 2>/dev/null | tr '\n' ' ')"
