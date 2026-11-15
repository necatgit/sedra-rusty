# End-to-end test: Sedra merged mining through the pool (M10 step 5c)

Local and isolated: two Sedra devnet nodes (sedra-rusty `m10-auxpow`, merged mining active from DAA 0), two Kaspa
devnet nodes (upstream rusty-kaspa) as the parent, Miningcore (branch `sedra-merged-mining`) with
`additionalMergedMining`, and a CPU stratum miner. Everything listens on loopback (the stratum port on the Windows
host's WSL interface only); no DNS seeding, no public network, no production system.

## Run

WSL (Ubuntu), Postgres 16 from `apt` (only `initdb`/`pg_ctl`/`psql` are used, as a normal user on port 5433).

1. `bash build_nodes.sh` builds `~/e2e-sedrad` (sedra-rusty) and `~/e2e-kaspad` (upstream rusty-kaspa).
2. `bash build_boot.sh` builds `e2e-boot` against both code bases (mines a few blocks so a fresh devnet is "synced").
3. `bash pg_setup.sh` creates the throwaway cluster `~/e2e-pgdata`, the Miningcore schema and the share partitions.
4. `bash start_nodes.sh` (also builds the miner once) and `bash start_peers.sh` (Miningcore waits for one peer).
5. Bootstrap: `~/e2e-boot-kaspa-bin grpc://127.0.0.1:16610 <kaspadev address> 3` and
   `~/e2e-boot-sedra-bin grpc://127.0.0.1:22610 <sedradev address> 3`.
6. Miningcore on Windows: set the stratum `listenAddress` in `config.e2e.json` to the Windows host IP on the WSL
   interface (`ip route show default` in WSL), then `dotnet src/Miningcore/bin/Debug/net10.0/Miningcore.dll -c
   config.e2e.json`.
7. Miner: `~/e2e-miner-bin <host ip>:5599 <kaspadev address>.rig1 a=<sedradev address> 150`.
8. `bash verify.sh` (P2P relay to the second Sedra node, blocks and share copies in the database), then
   `bash cleanup.sh`.

Addresses: `python addr.py kaspadev 22` / `python addr.py sedradev 44` (Kaspa-family bech32 with a self-check).
The miner computes kHeavyHash with sedra-rusty's own `kaspa-pow` and speaks the "large job" and the classic
(4 x u64) Kaspa stratum dialects.

## Result 2026-11-10

- Run 1: 327 shares accepted, 234 SDR aux blocks accepted, **19 rejected**: Miningcore threw while serializing the
  parent's blue work (`BigInteger` read hex starting with 8-f as negative). The bug is in the shared serializer, so it
  also affected ZKAS (latent in production: 0 occurrences in 72 h, today's mainnet blue work is outside the range).
  Fixed on Miningcore `dev` (`18862898`) and merged into the branch.
- Run 2: 559 shares, **401 of 401 SDR aux blocks accepted**, 0 exceptions, 498 Kaspa devnet blocks. The second
  Sedra node accepted all 638 blocks via relay (P2P with proofs). Database: 635 SDR blocks in `e2e-sedra` credited to
  the miner's SDR address; the `e2e-sedra` share copies carry the SDR address in `mpassword` (what PPLNS pays).
- Also found and fixed on the branch: the aux wallet host defaulted to the production ZKAS wallet IP (`7fbff17b`), and
  Sedra test-network address prefixes were not recognized (`2b6b16be`).

Not covered: ZKAS and SDR in the same coinbase against real nodes (no local ZKas node; covered by unit tests),
payouts (payment processing disabled), classification and maturity of the SDR blocks.

## Wallet test 2026-11-10: Go `sedrawallet` against the Rust node

`bash wallet_test.sh` (needs `~/repos/sedrad` at v2.0.0 and Go): one Rust devnet node with `--utxoindex`, merged
mining off; the Go wallet daemon `sedrawallet` from sedrad v2.0.0 connected to it. `walletcheck/` is a small client
that calls the daemon exactly as Miningcore's payout does (`GetBalance`, then `Send` with the password, change address
reused).

Result: devnet addresses, balance (available and pending), a CLI send (unsigned transaction, local signing, broadcast)
and the daemon's own `Send` all work. 65,000 SDR before, 64,992.9998 after: exactly the 7 SDR payout to an outside
address plus two fees of 0.0001; the 10 SDR sent to the wallet's second address stayed in the wallet. Both
transactions were mined, no errors in the node or daemon logs. The node binary was `m10-auxpow`, which contains `dev`
(the v0.1.0 candidate); without the activation flag its merged-mining code is inactive.
