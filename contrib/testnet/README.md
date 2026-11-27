# Public testnet kit (#13, SIP-0003)

Tools, configs and scripts for the public merged-mining testnet. The documents (participant guide, operator runbook,
pass-report template, design) are in the project repository, `necatgit/sedra-project`, directory `testnet/`.
**Running since 2026-11-27 10:00 UTC** (chain started from genesis on time): the Sedra team agreed on 2026-11-24 (SIP-0003 Q51); release `sedra-v0.2.0-testnet.1`
published as a pre-release 2026-11-25; start Tue 2026-12-01, announced by the Sedra team on 2026-11-26; the chain
started from genesis on 2026-11-27 10:00 UTC (same release and heights), so merged mining activates about 2026-12-01
([sedra-project#13](https://github.com/necatgit/sedra-project/issues/13#issuecomment-7192048626)).

| Path | What |
|---|---|
| `build_tools.sh` | builds the two tools below into `tools/target/release/` |
| `tools/` `sedra-testnet-miner` | native CPU miner over gRPC; `--sdm1` mines a block whose own coinbase carries a well-formed `SDM1` commitment (self-parent-ban test, SIP-0001 ยง4.5). Testnet and devnet only |
| `tools/` `sedra-testnet-monitor` | polls nodes and writes `nodes.csv`, `blocks.csv` (every block native or merged-mined, with its coinbase tag) and `agreement.csv` (does every node have the reference chain). Go sedrad nodes are asked through `sedractl://HOST:PORT` |
| `summary.py` | the monitor's CSVs as the numbers of the pass report |
| `status_page.py` | the public status page (SIP-0003 ง4.4): static `index.html` + `status.json` from the monitor's CSVs (phase and progress to ban start and activation, nodes and agreement, native vs merged-mined counts, share per tag, recent blocks). No payout scripts, no node errors. Run every minute by `systemd/sedra-testnet-status.timer`; any static web server serves the output |
| `faucet.sh` | sends testnet SDR from the faucet wallet (Go sedrawallet) to one `sedratest:` address |
| `systemd/` | units for a testnet node, the old Go node (criterion 3), the monitor and the status page, the Kaspa testnet-10 and private ZKas nodes, the pool (`sedra-testnet-pool`) and its three wallet daemons (`sedra-testnet-wallet-{kas,sdr,zkas}`, localhost only; secrets come from an `EnvironmentFile`, never from the unit), with `MemoryMax` |
| `pool/miningcore-testnet.json` | Miningcore (branch `sedra-merged-mining`): Kaspa testnet-10 parent, ZKas test chain and Sedra testnet as aux chains. Each `genesisHash` must be that chain's testnet genesis; the ZKas pool address must be shielded (`zkastest:p...`); `rewardCrossCheckUrl` is `off` on the parent and on the Sedra aux pool, which is written out for that reason |
| `dryrun/` | a private dry run of the whole kit on one host (loopback, own ports and data dir, activation override) |

The testnet release carries its activation score in `TESTNET_PARAMS` (0.2.0-testnet.1: 345,600; ban start
172,800). `--aux-pow-activation-override=<DAA>` replaces
it for dry runs only (allowed on devnet and testnet, refused on mainnet and simnet).
