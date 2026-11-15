# AGENTS.md: working rules for sedra-rusty

These rules apply to people and AI agents alike. They follow the project rules in
[necatgit/sedra-project](https://github.com/necatgit/sedra-project) (`AGENTS.md` there; public from late November 2026); this file adds
what is specific to node code. `SEDRA.md` lists every change from upstream.

## Consensus first

- The reference is **sedrad v2.0.0** (`4a1153d`), the version Sedra mainnet runs. Until Step 2, this node must accept
  and reject exactly the same blocks as that Go node.
- Expected values in tests come from **sedrad's Go code or real chain data**, never from this Rust code itself.
  A test that only compares the code with its own output proves nothing.
- Every consensus change needs tests for the valid case, the invalid case and the boundaries (for example the first and
  last DAA score of a window). Check that a new test fails when the rule is broken.
- Never change a rule that already applied to mined blocks; new nodes must still validate the chain.
- Keep upstream structure and names unless a change is needed. Mark Sedra changes with a `SEDRA:` comment and list them
  in `SEDRA.md`.

## Branches, commits, pushes

- Branch model (Git Flow, since 2026-11-14): `main` holds releases only, each a merge commit with a signed tag
  `sedra-vX.Y.Z`. `dev` is the integration branch; all work goes there or to a feature branch off `dev` (for example
  `m10-auxpow`) that merges back. A release branch `release-X.Y.Z` is cut from `dev`, takes only release fixes, docs
  and the version number, then merges into `main` (tagged) and back into `dev`, and is deleted. Unmodified upstream
  v1.0.1 is the tag `v1.0.1`.
- Push only after a successful build and the full test run, and report the real results in the commit message or issue.
- Commits are authored by **Nec** with the GitHub noreply address. Check `git config user.name` before committing.
- Ask first before bigger changes: consensus rules, the base version, protocol versions, or anything that affects how
  this node interacts with the live network.
- Never force-push, rewrite history or delete branches without explicit approval.

## Testing

```
cargo build --release --bin kaspad
cargo test --release --workspace --exclude kaspa-wasm --exclude kaspa-wallet-pskt
```

- Compare against the recorded baseline of unmodified v1.0.1. A failure that also happens on the tag `v1.0.1` is upstream,
  not ours; a new failure is ours and must be fixed or explained.
- Never claim something was built or tested unless it was actually run.

## Safety

- Never connect this node to Sedra or Kaspa mainnet peers, wallets or pools without explicit approval. Tests use local
  nodes, simnet/devnet or recorded data.
- Never use real keys or wallets in tests.
- Security fixes from later rusty-kaspa releases are tracked and back-ported deliberately, with a reference to the
  upstream commit.
