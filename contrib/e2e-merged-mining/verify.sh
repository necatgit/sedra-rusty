#!/bin/bash
# Checks after the end-to-end run: P2P sync of the merged-mined blocks and the pool's bookkeeping.
echo "--- sedra node 1 vs node 2 (synced over P2P from node 1)"
for f in ~/e2e-sedrad.log ~/e2e-sedrad2.log; do
  echo "$f: $(grep -c 'Accepted block' $f) accepted-block log lines, last: $(grep 'Accepted block' $f | tail -1 | cut -c1-140)"
done
grep -iE "invalid|error|reject" ~/e2e-sedrad2.log | tail -3 | cut -c1-200
echo "--- database"
export PGPASSWORD=e2e-local-only
P="/usr/lib/postgresql/16/bin/psql -h 127.0.0.1 -p 5433 -U miningcore -d miningcore -At -F |"
$P -c "SELECT poolid, type, status, count(*), count(DISTINCT miner), min(miner) FROM blocks GROUP BY 1,2,3 ORDER BY 1,2,3"
echo "--- shares per pool: miner / minerpass (the aux copy must carry the SDR address)"
$P -c "SELECT poolid, count(*), min(miner), min(mpassword), max(mpassword) FROM shares GROUP BY 1 ORDER BY 1" 2>&1 | head
$P -c "\d shares" | grep -i pass
