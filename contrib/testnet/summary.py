#!/usr/bin/env python3
"""SEDRA (#13): summarizes the CSV files of sedra-testnet-monitor into the numbers the pass report needs
(SIP-0003 section 3). Standard library only.

Usage: python3 summary.py DIR --activation DAA [--finality 86400] [--reference NAME]

Prints Markdown: blocks per UTC day (native, merged, red), blocks per coinbase tag (pool share), the first merged-mined
block, blocks carrying an SDM1 commitment in their own coinbase (must all lie before the ban start), node availability
and lag, and agreement (which node stopped having the reference chain, and when).
"""
import argparse
import csv
import datetime as dt
import os
from collections import Counter, defaultdict


def day(t):
    return dt.datetime.fromtimestamp(int(t), dt.timezone.utc).strftime("%Y-%m-%d")


def ts(t):
    return dt.datetime.fromtimestamp(int(t), dt.timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


def rows(path):
    if not os.path.exists(path):
        return []
    with open(path, newline="") as f:
        return list(csv.DictReader(f))


def tag(extra):
    # the node writes "<version>/<miner extra data>"; the pool or miner tag is what follows the first slash
    return extra.split("/", 1)[1] if "/" in extra else extra


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dir")
    ap.add_argument("--activation", type=int, required=True, help="merged-mining activation DAA score")
    ap.add_argument("--finality", type=int, default=86400, help="finality depth (testnet: 86400)")
    ap.add_argument("--reference", default=None, help="name of the reference node (default: first in nodes.csv)")
    a = ap.parse_args()
    ban_start = max(0, a.activation - 2 * a.finality)

    blocks = rows(os.path.join(a.dir, "blocks.csv"))
    nodes = rows(os.path.join(a.dir, "nodes.csv"))
    agree = rows(os.path.join(a.dir, "agreement.csv"))

    print(f"# Testnet monitor summary\n\nSource: `{a.dir}`. Activation DAA {a.activation}, ban start DAA {ban_start} "
          f"(activation - 2 x {a.finality}). Generated {ts(dt.datetime.now(dt.timezone.utc).timestamp())}.\n")

    # --- blocks (criterion 2) ---
    print("## Blocks (criterion 2)\n")
    per_day = defaultdict(Counter)
    for b in blocks:
        per_day[day(b["unix_time"])][b["kind"]] += 1
        per_day[day(b["unix_time"])]["red"] += int(b["red"])
    print("| UTC day | native | merged | red | total |\n|---|---|---|---|---|")
    for d in sorted(per_day):
        c = per_day[d]
        print(f"| {d} | {c['native']} | {c['merged']} | {c['red']} | {c['native'] + c['merged']} |")
    after = [b for b in blocks if int(b["daa_score"]) >= a.activation]
    merged = [b for b in blocks if b["kind"] == "merged"]
    early_merged = [b for b in merged if int(b["daa_score"]) < a.activation]
    print(f"\nBlocks recorded: {len(blocks)}; at or after activation: {len(after)} "
          f"(native {sum(b['kind'] == 'native' for b in after)}, merged {sum(b['kind'] == 'merged' for b in after)}).")
    if merged:
        first = min(merged, key=lambda b: int(b["daa_score"]))
        print(f"First merged-mined block: `{first['hash']}` DAA {first['daa_score']}, recorded {ts(first['unix_time'])}.")
    print(f"Merged-mined blocks below the activation score (must be 0): **{len(early_merged)}**.\n")

    print("### Share per coinbase tag, at or after activation (SIP-0001 section 8)\n")
    shares = Counter((tag(b["extra_data"]), b["kind"]) for b in after)
    total = max(1, len(after))
    print("| tag | kind | blocks | share |\n|---|---|---|---|")
    for (t, k), n in shares.most_common(15):
        print(f"| `{t or '(none)'}` | {k} | {n} | {100 * n / total:.1f}% |")

    # --- self-parent ban (criterion 1) ---
    print("\n## Blocks with an SDM1 commitment in their own coinbase (criterion 1, ban)\n")
    sdm1 = [b for b in blocks if "SDM1" in b["extra_data"]]
    late = [b for b in sdm1 if int(b["daa_score"]) >= ban_start]
    print(f"Accepted with SDM1: {len(sdm1)}; at or after the ban start (must be 0): **{len(late)}**.")
    for b in late[:10]:
        print(f"- `{b['hash']}` DAA {b['daa_score']}")

    # --- nodes (criteria 1, 3, 8) ---
    print("\n## Nodes (criteria 1, 3, 8)\n")
    names = list(dict.fromkeys(n["node"] for n in nodes))
    ref = a.reference or (names[0] if names else None)
    ref_daa = {n["unix_time"]: int(n["virtual_daa_score"]) for n in nodes if n["node"] == ref and n["status"] == "ok"}
    print("| node | polls | reachable | synced | last DAA | max lag behind reference (DAA) | version |\n|---|---|---|---|---|---|---|")
    for name in names:
        mine = [n for n in nodes if n["node"] == name]
        ok = [n for n in mine if n["status"] == "ok"]
        lag = [ref_daa[n["unix_time"]] - int(n["virtual_daa_score"]) for n in ok if n["unix_time"] in ref_daa]
        print(f"| {name} | {len(mine)} | {100 * len(ok) / max(1, len(mine)):.1f}% | "
              f"{100 * sum(n['is_synced'] == '1' for n in ok) / max(1, len(ok)):.1f}% | "
              f"{ok[-1]['virtual_daa_score'] if ok else '-'} | {max(lag) if lag else '-'} | "
              f"{ok[-1]['server_version'] if ok else '-'} |")

    print("\n## Agreement with the reference chain (criteria 1, 3, 8)\n")
    print("| node | checks | had the block | on its selected chain | first miss (DAA, time) |\n|---|---|---|---|---|")
    by_node = defaultdict(list)
    for r in agree:
        by_node[r["node"]].append(r)
    for name, rs in by_node.items():
        miss = [r for r in rs if r["present"] == "0" or r["is_chain_block"] == "0"]
        first = f"{miss[0]['daa_score']}, {ts(miss[0]['unix_time'])}" if miss else "none"
        print(f"| {name} | {len(rs)} | {sum(r['present'] == '1' for r in rs)} | "
              f"{sum(r['is_chain_block'] == '1' for r in rs)} | {first} |")
    print("\nAn upgraded node should never miss. A node without merged mining should miss from the first merged-mined "
          "block on, and never before it.")


if __name__ == "__main__":
    main()
