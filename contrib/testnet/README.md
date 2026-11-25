# Public testnet kit (#13, SIP-0003)

Tools, configs and scripts for the public merged-mining testnet. The documents (participant guide, operator runbook,
pass-report template, design) are in the project repository, `necatgit/sedra-project`, directory `testnet/`.
**Not launched**: the launch waits for the Sedra team's agreement (SIP-0003 Q51, Q52).

| Path | What |
|---|---|
| `build_tools.sh` | builds the two tools below into `tools/target/release/` |
| `tools/` `sedra-testnet-miner` | native CPU miner over gRPC; `--sdm1` mines a block whose own coinbase carries a well-formed `SDM1` commitment (self-parent-ban test, SIP-0001 §4.5). Testnet and devnet only |
| `tools/` `sedra-testnet-monitor` | polls nodes and writes `nodes.csv`, `blocks.csv` (every block native or merged-mined, with its coinbase tag) and `agreement.csv` (does every node have the reference chain). Go sedrad nodes are asked through `sedractl://HOST:PORT` |
| `summary.py` | the monitor's CSVs as the numbers of the pass report |
| `status_page.py`, `status_assets/` | the public status page (SIP-0003 §4.4), in the puddlepool.com look with a TESTNET banner: static `index.html` + `status.json` from the monitor's CSVs (phase with countdowns to ban start and activation, DAA, difficulty, hashrate estimate, nodes and agreement, native vs merged-mined counts per day and per tag, SIP-0003 criteria 1/2/3/8 live and 4-7 from the operator's `--criteria` file, join links, recent blocks). No payout scripts, no node errors. Works without JavaScript. Run every minute by `systemd/sedra-testnet-status.timer`; `--publish` pushes it to the web host. Tests: `python3 -m unittest discover -s contrib/testnet/tests` |
| `faucet.sh` | sends testnet SDR from the faucet wallet (Go sedrawallet) to one `sedratest:` address |
| `systemd/` | units for a testnet node, the old Go node (criterion 3), the monitor and the status page (with its SSH push connection), with `MemoryMax` |
| `web/Caddyfile` | the web host for the status page (testnet.puddlepool.com on the VPS): static files, automatic Let's Encrypt, no client IPs in the log |
| `pool/miningcore-testnet.json` | Miningcore (branch `sedra-merged-mining`): Kaspa testnet-10 parent, Sedra testnet aux chain. `genesisHash` must be the testnet genesis |
| `dryrun/` | a private dry run of the whole kit on one host (loopback, own ports and data dir, activation override) |

The testnet release carries its activation score in `TESTNET_PARAMS` (0.2.0-testnet.1: 345,600; ban start
172,800). `--aux-pow-activation-override=<DAA>` replaces
it for dry runs only (allowed on devnet and testnet, refused on mainnet and simnet).

## Status page

The monitor and `status_page.py` run at home next to the nodes; the page is served from the VPS (seed2), so it stays
reachable when the home network is not, and the public web host never needs access to a node or to the home network.

```
home: monitor -> CSVs -> status_page.py (every minute) --rsync over SSH--> VPS: /var/www/sedra-testnet <- Caddy (HTTPS)
```

- Data: the monitor writes `nodes.csv`, `blocks.csv`, `agreement.csv` and `network.csv` (difficulty and hashrate
  estimate of the reference node). The page shows what it last received with its time; when the data is more than five
  minutes old, it says so (in the page, and live in the browser when JavaScript is on).
- Push key: a dedicated ed25519 key, owned by the `sedra` user on the monitor host (`/etc/sedra-testnet/statuspush_ed25519`),
  allowed on the VPS only for one user and one directory:
  `restrict,port-forwarding,permitopen="127.0.0.1:22210",command="rrsync -wo /var/www/sedra-testnet" ssh-ed25519 AAAA... statuspush`
  in `~statuspush/.ssh/authorized_keys` (`rrsync` from the rsync package). The key cannot open a shell, read files,
  write outside that directory, or forward to anything but seed2's local gRPC (seed2 runs without `--unsaferpc`). No
  `from=`: the home line has a dynamic address.
- One connection: `systemd/sedra-testnet-statuspush.service` keeps a single SSH master connection open. It carries the
  monitor's tunnel to seed2's gRPC (`--node seed2=grpc://127.0.0.1:22220`) and the push (one `rsync --delay-updates`
  per minute) goes over its socket. Opening a new SSH connection every minute from home could trip
  the home router's intrusion prevention, which blocked port 22 to the VPS after about eight connections in a few
  minutes on 2026-11-24. Without a live master the page is built but not pushed (`STATUS_PUSH_SOCKET`).
- Web host: `web/Caddyfile` (Caddy from Debian, automatic Let's Encrypt; TCP 80 and 443 open; DNS A/AAAA record for the
  name). The CSP in it carries the hash of the page's one inline script; the tests check that they match.
- Operator criteria: `--criteria /etc/sedra-testnet/criteria.json` records criteria 4-7 (and may override 1-3 and 8,
  for example with the ban test's result) with a note and an `https://` evidence link; format in
  `status_assets/criteria.example.json` (an example, not real results).
