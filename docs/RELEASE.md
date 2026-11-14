# Releases: build, verify, rebuild

A release of this node is meant to be checked, not trusted. Anyone can rebuild it from the tagged commit and must get
the same bytes.

## What a release contains

`sedra-rusty-<release>-linux-amd64.tar.gz` with:

| File | Content |
|---|---|
| `kaspad` | The Sedra node (the binary keeps its upstream name for now, see `SEDRA.md`) |
| `SEDRA.md` | Every change from upstream rusty-kaspa v1.0.1 |
| `RELEASE.md` | This file |
| `BUILDINFO` | Release, commit and compiler version it was built from |
| `LICENSE` | ISC, as upstream |

and `SHA256SUMS` with the checksum of the archive and of `kaspad`.

`kaspad --version` prints the release, the upstream base and the commit, for example
`0.1.0 (rusty-kaspa v1.0.1, commit 9cb74efc1234)`. Peers see the upstream user agent `kaspad:1.0.1`, as before.

## Verify a download

```
sha256sum -c SHA256SUMS
```

This only shows that the file arrived intact. That the binary really comes from the published source is shown by
rebuilding it (next section) and comparing the checksums. Results of independent rebuilds are collected in the release
issue; a match by someone other than the publisher is worth more than any signature.

Release tags carry the prefix `sedra-` (for example `sedra-v0.1.0`). This repository contains the history of upstream
rusty-kaspa, including its own tags `v0.1.0` and later, so a plain version tag would name a different commit.

## Verify the tag signature

Release tags are signed by Nec with an SSH key (Ed25519), fingerprint
`SHA256:Eh/4CtYZPvHjYldtNexGX4RLCsz/Mi5MsBZDLKTRvUc`. The key is published at <https://puddlepool.com/keys.html> and
listed as a signing key on the GitHub account `necatgit`:

```
curl -sO https://puddlepool.com/keys/allowed_signers
git -c gpg.ssh.allowedSignersFile=allowed_signers verify-tag sedra-v0.1.0
```

A good signature shows who made the tag. Whether the binaries match the source is shown only by rebuilding them
(next section).

## Rebuild a release yourself

Requirements: Linux x86_64, git, and Docker or Podman. One build took about 5 minutes and 1.5 GB of disk on an 8-thread machine (measured natively; the container adds the image build and the dependency download).

```
git clone <repository> sedra-rusty && cd sedra-rusty
git checkout <tag>
contrib/reproducible/build-in-docker.sh
cat dist/SHA256SUMS
```

Compare `dist/SHA256SUMS` with the published one. Both lines must match.

`build-in-docker.sh` builds the image from `contrib/reproducible/Dockerfile`: the official Rust 1.99.1 image pinned by
digest, with Debian packages (compilers, clang, protoc, cmake) from a fixed snapshot of snapshot.debian.org. Then it
runs `contrib/reproducible/build.sh` inside it, which:

- refuses to build from a working tree with uncommitted changes;
- builds `kaspad` with `cargo build --release --locked` (exact dependency versions from `Cargo.lock`);
- removes build paths from the binary (`--remap-path-prefix`, `-ffile-prefix-map`), fixes the embedded commit at 12
  characters, and sets `SOURCE_DATE_EPOCH` to the commit time;
- packs the archive with sorted names, fixed times and owners, and no gzip timestamp.

Running `contrib/reproducible/build.sh` directly on a host also works and is reproducible on that host, but the binary then
depends on the host's C and C++ compilers (RocksDB and secp256k1 are C/C++ code), so its checksum will usually differ
from the published one.

## How far this has been tested

- 2026-11-09, commit `f024e02a`: `build.sh` run directly on one host (Ubuntu 24.04, WSL) from two clones in different
  directories, both checked out as a detached HEAD like a release tag: identical `SHA256SUMS` (archive `108a2966…`,
  binary `98cf0e4f…`). This shows that build paths, timestamps and the clone do not reach the output.
- 2026-11-10, commit `527b4ca5` (branch `release-0.1.0`): the container route `build-in-docker.sh` with rootless
  Podman 4.9.3 on the dev box (WSL), from a fresh clone at a detached HEAD: build 5 min, archive `94e80d7e…`, binary
  `ec71ba49…`, `kaspad --version` = `0.1.0 (rusty-kaspa v1.0.1, commit 527b4ca537e7)`. Two fixes were needed for
  Podman: a fully qualified base image name and `--userns=keep-id`.
- 2026-11-14, tag `sedra-v0.1.0` (`d20f07be`): `build-in-docker.sh` on two different machines, Ubuntu 24.04 with
  Podman 4.9.3 (WSL) and Ubuntu 26.04 with Podman 5.7.0 (a VM): identical `SHA256SUMS` (archive `04cd6669…`, binary
  `3113bfe4…`). In 0.1.0 the script ended with exit code 1 after a complete build, because its last step,
  `kaspad --version`, exits with 1 after printing (upstream behaviour); fixed in 0.1.1.

## If your checksum differs

Open an issue with your `dist/SHA256SUMS`, `dist/sedra-rusty-*/BUILDINFO`, the output of `docker version` (or
`podman version`) and `uname -a`. A mismatch is a finding, not a mistake: either the build is not as reproducible as
claimed, or the published binary is not what the source produces. Both must be fixed before the release is used.

## Making a release (maintainer)

1. Full test run on the release commit (`cargo test --release --no-fail-fast --workspace --exclude kaspa-wasm
   --exclude kaspa-wallet-pskt`), result recorded in the release notes.
2. Cut the release branch `release-X.Y.Z` from `dev`; set the release in `core/src/kaspad_env.rs` (`sedra_release`)
   there. Only release fixes and docs go on it. An urgent fix while `dev` holds unreleased work goes on a hotfix
   branch `hotfix-X.Y.Z` cut from `main` instead, and follows the same steps.
3. Merge it into `main` as a merge commit (`git merge --no-ff`) and sign the tag `sedra-vX.Y.Z` on that commit with the
   release key. The key stays on its own machine: the tag text is built where the repository is, signed on the key's machine
   with `ssh-keygen -Y sign -f <key> -n git`, and the signature is appended with `git mktag`; `verify-tag` must show a good
   signature before the tag is pushed. Then merge the release branch back into `dev` and delete it.
4. `contrib/reproducible/build-in-docker.sh` on two different machines; both `SHA256SUMS` must match.
5. Publish the archive, `SHA256SUMS` and the release notes; open the release issue for independent rebuilds.
