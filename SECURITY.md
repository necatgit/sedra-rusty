# Security policy

This repository is the Rust node for the Sedra network (`sedra-rusty`), maintained by Nec. A flaw in a node can cost
money: please report it privately first and give us time to fix it before it becomes public.

## Report a vulnerability

Use GitHub's private reporting: the **Security** tab of this repository, then **Report a vulnerability**. Only the
maintainer sees the report.

Please include what you found, how to reproduce it (a test, a log or a sequence of messages), which version or commit
you used, and what an attacker could do with it. Do not test against nodes, pools or services you do not run
yourself.

Do not open a public issue for a vulnerability, and do not post details elsewhere until a fix is released.

## What happens next

- We confirm that we received the report within 7 days.
- We tell you whether we can reproduce it and how we plan to fix it.
- A fix is released as a new version with a note in `SEDRA.md` and the release notes. You are credited by the name
  you choose, or not at all if you prefer.
- Details are published once operators have had a fair chance to upgrade, normally at most 90 days after the report,
  earlier if the flaw is already being exploited or public.

This is a small project run by one maintainer. There is no bug bounty.

## What is in scope

| In scope here | Report it elsewhere |
|---|---|
| This node: consensus, P2P, RPC, and everything changed from upstream (listed in `SEDRA.md`) | A flaw that also exists in upstream rusty-kaspa: report it to the Kaspa developers as well, following their policy |
| The release process: builds, checksums, signatures (`docs/RELEASE.md`) | The Go node `sedrad`, wallets and services of the Sedra team: report it to the Sedra team |

If you are not sure where something belongs, report it here and we will forward it with your agreement.

## Supported versions

Only the latest release gets security fixes. The signing key for release tags is published at
<https://puddlepool.com/keys.html>.
