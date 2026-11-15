#!/bin/bash
# Throwaway Postgres cluster for the local end-to-end test (port 5433, ~/e2e-pgdata). Delete with: pg_ctl stop; rm -rf ~/e2e-pgdata
set -e
B=/usr/lib/postgresql/16/bin
D=~/e2e-pgdata
if [ ! -d $D ]; then
  echo "e2e-local-only" > /tmp/e2e-pgpass
  $B/initdb -D $D -U miningcore --pwfile=/tmp/e2e-pgpass -A scram-sha-256 >/dev/null
  rm /tmp/e2e-pgpass
fi
$B/pg_ctl -D $D -l ~/e2e-pg.log -o "-p 5433 -k /tmp -c listen_addresses=127.0.0.1" status >/dev/null 2>&1 || \
  $B/pg_ctl -D $D -l ~/e2e-pg.log -o "-p 5433 -k /tmp -c listen_addresses=127.0.0.1" start -w
export PGPASSWORD=e2e-local-only
P="$B/psql -h 127.0.0.1 -p 5433 -U miningcore -v ON_ERROR_STOP=1 -q"
$P -d postgres -tc "SELECT 1 FROM pg_database WHERE datname='miningcore'" | grep -q 1 || $P -d postgres -c "CREATE DATABASE miningcore"
$P -d miningcore -tc "SELECT to_regclass('public.shares')" | grep -q shares || \
  $P -d miningcore -f /mnt/c/Users/Nec/repos/miningcore/src/Miningcore/Persistence/Postgres/Scripts/createdb.sql
for pool in e2e e2e-sedra e2e-zkas; do
  $P -d miningcore -c "CREATE TABLE IF NOT EXISTS \"shares_${pool}\" PARTITION OF shares FOR VALUES IN ('${pool}');"
done
$P -d miningcore -c "\dt" | head -30
