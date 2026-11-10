# sedra-rusty: a Rust node for Sedra

A full node for the [Sedra](https://sedracoin.com) network (SDR), built on
[rusty-kaspa](https://github.com/kaspanet/rusty-kaspa) **v1.0.1**, the Rust node of Kaspa.

Sedra's own node, `sedrad`, is a copy of Kaspa's Go node v0.12.15, which Kaspa no longer maintains. This node runs
**exactly Sedra mainnet's current rules** (those of sedrad v2.0.0) on the maintained Rust codebase, next to the
existing Go nodes. It is not a fork and changes nothing on the network.

It is step 1 of the [Sedra merged-mining proposal (SIP-0001)](https://github.com/necatgit/sedra-sips). Step 2, merged
mining with Kaspa, is a separate hard fork that needs agreement first; nothing of it is in this node's `dev` branch.

## Status

**Work in progress. Not yet released.** Milestones of step 1:

| Milestone | Result |
|---|---|
| M0 | Sedra's genesis hash reproduced with rusty-kaspa's hashing rules |
| M1 | Sedra network parameters on rusty-kaspa v1.0.1 (genesis, names, address prefixes, ports, seeds) |
| M2 | Block rewards identical to sedrad (813 values from sedrad's own code) |
| M3 | 2001 real Sedra mainnet headers decode and validate offline |
| M4 | P2P handshake with a Go node (protocol version 5) |
| M5 | Full sync from a Go node, then follows the tip with no difference |
| M6 | The two v2.0.0 relaunch rules (difficulty reset, treasury outputs) behave exactly like sedrad |
| M7 | Same UTXO commitment, supply, balances and UTXOs as the Go node |
| M8 | Blocks mined by this node are accepted by a Go node and vice versa (private test network) |
| M9 | Seven days next to Go nodes on mainnet data with no divergence: **running until 2026-11-14** |

Security fixes from later rusty-kaspa releases are carried over and listed in `SEDRA.md`; the full test suite passes.
Evidence for each milestone is in the research notes of the project.

## What differs from upstream

Everything is listed in [SEDRA.md](SEDRA.md), with the sedrad source of each Sedra value. In short: Sedra's genesis,
network names, address prefixes (`sedra:`), ports (P2P 22111, RPC 22110), DNS seeders and block-reward table; later
Kaspa features (Crescendo, 10 blocks per second) switched off; the Go nodes' P2P protocol version 5 accepted; the
v2.0.0 relaunch rules; and the security back-ports.

The binary is still called `kaspad`, and its data lives in `~/.rusty-kaspa/sedra-mainnet/` (separate from a Kaspa node's
`kaspa-mainnet/`). The upstream documents in `docs/` (Crescendo, testnet 10) describe Kaspa and do not apply to Sedra.

## Build

Linux, Rust via rustup (the version is pinned in `rust-toolchain.toml`), and
`build-essential clang llvm libclang-dev pkg-config libssl-dev protobuf-compiler cmake`:

```
cargo build --release --bin kaspad
./target/release/kaspad --utxoindex
```

Reproducible release builds and how to verify them: [docs/RELEASE.md](docs/RELEASE.md).

## Tests

```
cargo test --release --no-fail-fast --workspace --exclude kaspa-wasm --exclude kaspa-wallet-pskt
```

Sedra-specific tests are named `sedra_*`. Their expected values come from sedrad's Go code or from real chain data,
never from this code itself.

## Contributing and reporting

Issues and reviews are welcome, especially independent rebuilds of releases and comparisons with a Go node. Security
problems: please report them privately first, as described in [SECURITY.md](SECURITY.md).

Working rules for changes: [AGENTS.md](AGENTS.md).

## Who

Maintained by **Nec** ([@necatgit](https://github.com/necatgit)), who also runs the PuddlePool mining pool and holds SDR
(disclosure in the [SIP repository](https://github.com/necatgit/sedra-sips)). Developed together with an AI (Claude, by
Anthropic); commits it wrote carry a `Co-Authored-By: Claude` line.

## Licence

ISC, as upstream rusty-kaspa. The original copyright notices are kept. The upstream README is kept as
[docs/README-rusty-kaspa.md](docs/README-rusty-kaspa.md).
