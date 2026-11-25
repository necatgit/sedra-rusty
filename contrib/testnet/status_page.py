#!/usr/bin/env python3
"""SEDRA (#13): public status page of the Sedra testnet, built from the CSV files of sedra-testnet-monitor.

SIP-0003 section 4.4 promises "a status page with recent blocks". This script writes a static `index.html`, a
`status.json` and the page's assets (`status_assets/`: stylesheet, a small optional script, logos) into an output
directory that any web server can serve. It runs once per call (a systemd timer calls it every minute, see
systemd/sedra-testnet-status.timer); it needs only the Python standard library and never talks to a node itself, so
the page can be built next to the monitor and copied to a public web host (`--publish`).

Usage:
  python3 status_page.py MONITOR_DIR --out WWW_DIR [--activation 345600] [--finality 86400]
         [--old old-go] [--seed host:22211 ...] [--pool stratum+tcp://host:15555] [--report-url URL]
         [--criteria criteria.json] [--publish user@host:/var/www/testnet/]

Shown: a TEST NETWORK banner; the phase with live countdowns to the ban start and the activation; tip DAA score, DAA
rate, difficulty and hashrate estimate; every node with its state and whether it still has the reference chain;
blocks native and merged-mined (all time, last 24 h, per day, per coinbase tag); the SIP-0003 pass criteria (1, 2, 3
and 8 from the monitor's data, 4 to 7 from the operator's `--criteria` file); how to join (guide, faucet, release,
checksums, seeds, pool); the most recent blocks. Not shown on purpose: payout scripts (only how many distinct ones),
node errors (they can contain addresses) and anything about hosts beyond the seed and pool addresses given.

blocks.csv grows by about 86,400 rows a day, so it and agreement.csv are read incrementally: a small state file in the
output directory keeps the byte offsets and the counters. Delete it to recount from the start.
"""
import argparse
import csv
import datetime as dt
import hashlib
import html
import io
import json
import os
import shutil
import subprocess
import tempfile
from collections import Counter, deque

STATE = ".status-state.json"
ASSETS = "status_assets"
RECENT = 30
STALE_SECONDS = 300
TWO_WEEKS = 14 * 86400
GITHUB = "https://github.com/necatgit"
DEFAULTS = {
    "guide_url": f"{GITHUB}/sedra-project/blob/main/testnet/GUIDE.md",
    "faucet_url": f"{GITHUB}/sedra-project/issues/new?template=testnet-report.md",
    "release_url": f"{GITHUB}/sedra-rusty/releases/tag/sedra-v0.2.0-testnet.1",
    "checksums_url": f"{GITHUB}/sedra-project/blob/main/release/v0.2.0-testnet.1-SHA256SUMS",
    "sip_url": f"{GITHUB}/sedra-project/blob/main/proposals/SIP-0003-public-testnet.md",
    "home_url": "https://puddlepool.com",
}
CRITERIA = [
    (1, "Activation at the announced DAA score without a split; the self-parent ban starts 2 × finality depth earlier"),
    (2, "Merged-mined and natively mined blocks side by side, accepted by every upgraded node"),
    (3, "Old nodes follow before activation and stop at the first merged-mined block"),
    (4, "A node started from scratch after activation syncs, including the pruning proof with merged-mined headers"),
    (5, "A pool pays both coins (Kaspa testnet and Sedra testnet) and per-miner addresses work"),
    (6, "The Go sedrawallet sends and receives"),
    (7, "At least one participant outside this project runs a node or mines and reports"),
    (8, "At least two weeks after activation without a consensus failure"),
    (9, "Proposed addition: one Kaspa testnet-10 parent carries both a ZKas and a Sedra commitment; both chains accept, "
        "no block rejected because of the other tag, no double credit"),
]
# state of a criterion -> label shown and CSS class
STATES = {
    "waiting": ("waiting", "wait"),
    "running": ("in progress", "run"),
    "met": ("met so far", "ok"),
    "passed": ("passed", "ok"),
    "failed": ("failed", "bad"),
    "open": ("not measured yet", "wait"),
}


def now():
    return int(dt.datetime.now(dt.timezone.utc).timestamp())


def utc(t):
    return dt.datetime.fromtimestamp(int(t), dt.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")


def utc_s(t):
    return dt.datetime.fromtimestamp(int(t), dt.timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


def span(seconds):
    """A duration as '2 d 4 h', '3 h 12 min' or '45 s'."""
    s = max(0, int(seconds))
    d, h, m = s // 86400, s % 86400 // 3600, s % 3600 // 60
    if d:
        return f"{d} d {h} h"
    if h:
        return f"{h} h {m} min"
    if m:
        return f"{m} min"
    return f"{s} s"


def si(v, unit=""):
    """1234567 -> '1.23 M' (+ unit)."""
    if v is None:
        return "-"
    for p, n in ((1e15, "P"), (1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "k")):
        if abs(v) >= p:
            return f"{v / p:.2f} {n}{unit}"
    return f"{v:.0f} {unit}".rstrip() if unit else f"{v:.0f}"


def tag(extra):
    # the node writes "<version>/<miner extra data>"; the pool or miner tag is what follows the first slash
    return extra.split("/", 1)[1] if "/" in extra else extra


def tail_rows(path, max_bytes=2_000_000):
    """The last rows of a CSV file (the header is read separately), without reading the whole file."""
    if not os.path.exists(path):
        return []
    with open(path, "rb") as f:
        header = f.readline().decode()
        size = os.fstat(f.fileno()).st_size
        start = max(f.tell(), size - max_bytes)
        f.seek(start)
        data = f.read().decode(errors="replace")
    if start > len(header):
        data = data.split("\n", 1)[1] if "\n" in data else ""  # drop the partial first line
    return list(csv.DictReader(io.StringIO(header + data)))


def fresh_state():
    return {"offset": 0, "header": None, "kinds": {}, "tags": {}, "days": {}, "first_merged": None, "sdm1": [],
            "after_activation": {}, "reached": {}, "agree_offset": 0, "agree_header": None, "first_miss": {},
            "zkas_offset": 0, "zkas_header": None, "zkas_kinds": {}, "zkas_days": {},
            "shared_offset": 0, "shared_header": None, "shared_count": 0, "shared_first": None}


def load_state(path):
    st = fresh_state()
    try:
        with open(path) as f:
            st.update(json.load(f))
    except (OSError, ValueError):
        pass
    return st


def read_new(path, st, okey, hkey):
    """The complete CSV rows written since the last call; keeps the byte offset in st[okey], the header in st[hkey].
    A file that shrank was replaced (reset): returns None so the caller starts over."""
    if not os.path.exists(path):
        return []
    with open(path, "rb") as f:
        size = os.fstat(f.fileno()).st_size
        if size < st[okey]:
            return None
        if st[okey] == 0:
            st[hkey] = f.readline().decode().strip()
            st[okey] = f.tell()
        f.seek(st[okey])
        data = f.read()
    end = data.rfind(b"\n") + 1  # only complete lines; the monitor may be writing the last one
    st[okey] += end
    return list(csv.DictReader(io.StringIO(st[hkey] + "\n" + data[:end].decode(errors="replace"))))


def count_blocks(path, st, a, ban_start):
    new = read_new(path, st, "offset", "header")
    if new is None:  # blocks.csv replaced: recount everything
        keep = {k: st[k] for k in ("agree_offset", "agree_header", "first_miss", "zkas_offset", "zkas_header",
                                   "zkas_kinds", "zkas_days", "shared_offset", "shared_header", "shared_count",
                                   "shared_first")}
        st.clear()
        st.update(fresh_state() | keep)
        new = read_new(path, st, "offset", "header") or []
    for b in new:
        k = b["kind"]
        daa = int(b["daa_score"])
        st["kinds"][k] = st["kinds"].get(k, 0) + 1
        key = f"{tag(b['extra_data']) or '(none)'}|{k}"
        st["tags"][key] = st["tags"].get(key, 0) + 1
        d = dt.datetime.fromtimestamp(int(b["unix_time"]), dt.timezone.utc).strftime("%Y-%m-%d")
        st["days"].setdefault(d, {})
        st["days"][d][k] = st["days"][d].get(k, 0) + 1
        if daa >= a.activation:
            st["after_activation"][k] = st["after_activation"].get(k, 0) + 1
        for name, target in (("ban", ban_start), ("activation", a.activation)):
            if daa >= target and name not in st["reached"]:
                st["reached"][name] = int(b["unix_time"])
        if k == "merged" and (st["first_merged"] is None or daa < st["first_merged"]["daa_score"]):
            st["first_merged"] = {"hash": b["hash"], "daa_score": daa, "unix_time": int(b["unix_time"])}
        if "SDM1" in b["extra_data"]:
            st["sdm1"] = (st["sdm1"] + [{"hash": b["hash"], "daa_score": daa}])[-20:]


def count_zkas(path, st):
    """ZKas blocks (monitor --zkas, zkas_blocks.csv): kinds and per day, read incrementally like blocks.csv."""
    new = read_new(path, st, "zkas_offset", "zkas_header")
    if new is None:
        st.update(zkas_offset=0, zkas_header=None, zkas_kinds={}, zkas_days={})
        new = read_new(path, st, "zkas_offset", "zkas_header") or []
    for b in new:
        k = b["kind"]
        st["zkas_kinds"][k] = st["zkas_kinds"].get(k, 0) + 1
        d = dt.datetime.fromtimestamp(int(b["unix_time"]), dt.timezone.utc).strftime("%Y-%m-%d")
        st["zkas_days"].setdefault(d, {})
        st["zkas_days"][d][k] = st["zkas_days"][d].get(k, 0) + 1


def count_shared(path, st):
    """Kaspa parents that produced both a Sedra and a ZKas block (monitor's shared_parents.csv)."""
    new = read_new(path, st, "shared_offset", "shared_header")
    if new is None:
        st.update(shared_offset=0, shared_header=None, shared_count=0, shared_first=None)
        new = read_new(path, st, "shared_offset", "shared_header") or []
    for r in new:
        st["shared_count"] += 1
        if st["shared_first"] is None:
            st["shared_first"] = {"unix_time": int(r["unix_time"]), "parent_prefix": r["parent_prefix"],
                                  "sedra_hash": r["sedra_hash"], "zkas_hash": r["zkas_hash"]}


def count_agreement(path, st):
    new = read_new(path, st, "agree_offset", "agree_header")
    if new is None:
        st.update(agree_offset=0, agree_header=None, first_miss={})
        new = read_new(path, st, "agree_offset", "agree_header") or []
    for r in new:
        if (r["present"] == "0" or r["is_chain_block"] == "0") and r["node"] not in st["first_miss"]:
            st["first_miss"][r["node"]] = {"daa_score": int(r["daa_score"]), "unix_time": int(r["unix_time"])}


def load_criteria(path):
    """Operator file for the criteria the monitor cannot measure: {"4": {"status": "met", "note": "...",
    "link": "https://..."}, ...}. Status is one of waiting, running, met, passed, failed, open."""
    if not path:
        return {}
    try:
        with open(path) as f:
            data = json.load(f)
    except (OSError, ValueError):
        return {}
    out = {}
    for k, v in data.items():
        if isinstance(v, dict) and v.get("status") in STATES:
            link = str(v.get("link", ""))
            out[str(k)] = {"status": v["status"], "note": str(v.get("note", ""))[:300],
                           "link": link if link.startswith("https://") else ""}
    return out


def criteria_status(s, st, a, manual):
    """Live status of the SIP-0003 criteria. 1, 2, 3 and 8 come from the monitor's data (the operator file may still
    override them, e.g. with the ban test's result), 4 to 7 from the operator file. 9 (proposed addition, ZKas in the
    same coinbase) is live for the shared parents; rejections and double credit come from the operator file."""
    daa, act, ban = s["virtual_daa_score"], a.activation, s["ban_start"]
    old = set(a.old)
    upgraded_miss = {n: m for n, m in st["first_miss"].items() if n not in old}
    late_sdm1 = len(s["sdm1_blocks_after_ban_start"])
    after = st["after_activation"]
    first = st["first_merged"]
    out = {}

    if late_sdm1:
        out[1] = ("failed", f"{late_sdm1} block(s) with an SDM1 coinbase accepted after the ban start")
    elif daa < ban:
        out[1] = ("waiting", f"ban start at DAA {ban:,}, activation at DAA {act:,}")
    elif daa < act:
        out[1] = ("running", "self-parent ban active, no SDM1 coinbase accepted since; activation ahead")
    elif upgraded_miss:
        n, m = sorted(upgraded_miss.items(), key=lambda kv: kv[1]["daa_score"])[0]
        out[1] = ("failed", f"upgraded node {n} missed a reference chain block at DAA {m['daa_score']:,}")
    else:
        out[1] = ("met", "activation reached; every upgraded node has the reference chain; no SDM1 coinbase after the ban start")

    if daa < act:
        out[2] = ("waiting", "starts at the activation")
    elif after.get("merged") and after.get("native"):
        out[2] = ("met", f"since activation: {after['native']:,} native, {after['merged']:,} merged-mined"
                  + ("" if not upgraded_miss else "; but an upgraded node disagreed"))
        if upgraded_miss:
            out[2] = ("failed", out[2][1])
    else:
        out[2] = ("running", f"since activation: {after.get('native', 0):,} native, {after.get('merged', 0):,} merged-mined; both kinds needed")

    if not old:
        out[3] = ("open", "no old node configured on the monitor")
    else:
        notes, state = [], "met"
        for n in sorted(old):
            m = st["first_miss"].get(n)
            if not first:
                if m:
                    notes.append(f"{n} lost the chain at DAA {m['daa_score']:,} before any merged-mined block")
                    state = "failed"
                else:
                    notes.append(f"{n} follows the chain")
                    state = "waiting" if state != "failed" else state
            elif m and m["daa_score"] >= first["daa_score"]:
                notes.append(f"{n} stopped following at DAA {m['daa_score']:,} (first merged-mined block: {first['daa_score']:,})")
            elif m:
                notes.append(f"{n} lost the chain at DAA {m['daa_score']:,}, before the first merged-mined block")
                state = "failed"
            else:
                notes.append(f"{n} has not stopped yet (checked 100 DAA deep; first merged-mined block at {first['daa_score']:,})")
                state = "running" if state != "failed" else state
        out[3] = (state, "; ".join(notes))

    for n in (4, 5, 6, 7):
        out[n] = ("open", "recorded by the operator")

    reached = st["reached"].get("activation")
    if not reached:
        out[8] = ("waiting", "the two weeks start at the activation")
    elif upgraded_miss:
        out[8] = ("failed", "an upgraded node lost the reference chain; see the operator's report")
    else:
        days = (s["data_time"] - reached) / 86400
        out[8] = ("passed" if days >= 14 else "running", f"day {min(days, 99):.1f} of 14 since the activation")

    z = s["zkas"]
    if not z:
        out[9] = ("open", "no ZKas node configured on the monitor")
    elif s["shared_parents"]["count"]:
        out[9] = ("met", f"{s['shared_parents']['count']:,} Kaspa parents produced both a Sedra and a ZKas block; "
                         "rejections and double credit are checked in the logs and the pool's accounting")
    elif daa < act:
        out[9] = ("waiting", f"Sedra merged mining starts at the activation; ZKas blocks so far: {sum(z['blocks_all_time'].values()):,}")
    else:
        out[9] = ("running", "no Kaspa parent with both a Sedra and a ZKas block seen yet")

    rows = []
    for n, text in CRITERIA:
        state, note = out[n]
        link = ""
        if str(n) in manual:
            m = manual[str(n)]
            state, note, link = m["status"], m["note"] or note, m["link"]
        rows.append({"n": n, "text": text, "status": state, "note": note, "link": link})
    return rows


def build(a):
    os.makedirs(a.out, exist_ok=True)
    state_path = os.path.join(a.out, STATE)
    st = load_state(state_path)
    ban_start = max(0, a.activation - 2 * a.finality)
    count_blocks(os.path.join(a.monitor_dir, "blocks.csv"), st, a, ban_start)
    count_agreement(os.path.join(a.monitor_dir, "agreement.csv"), st)
    count_zkas(os.path.join(a.monitor_dir, "zkas_blocks.csv"), st)
    count_shared(os.path.join(a.monitor_dir, "shared_parents.csv"), st)
    t_now = now()

    # nodes: the newest row per node, plus the DAA rate of the reference node over the last hour
    node_rows = tail_rows(os.path.join(a.monitor_dir, "nodes.csv"))
    latest, order = {}, []
    for r in node_rows:
        if r["node"] not in latest:
            order.append(r["node"])
        latest[r["node"]] = r
    ref = order[0] if order else None
    ref_ok = [r for r in node_rows if r["node"] == ref and r["status"] == "ok"]
    daa = int(ref_ok[-1]["virtual_daa_score"]) if ref_ok else 0
    rate = None
    hour = [r for r in ref_ok if int(r["unix_time"]) >= int(ref_ok[-1]["unix_time"]) - 3600] if ref_ok else []
    if len(hour) >= 2 and int(hour[-1]["unix_time"]) > int(hour[0]["unix_time"]):
        rate = (int(hour[-1]["virtual_daa_score"]) - int(hour[0]["virtual_daa_score"])) / (
            int(hour[-1]["unix_time"]) - int(hour[0]["unix_time"]))
    data_time = max((int(r["unix_time"]) for r in node_rows), default=0)

    # network: difficulty and hashrate estimate of the reference node (monitor's network.csv, may be absent)
    net_rows = tail_rows(os.path.join(a.monitor_dir, "network.csv"), 500_000)
    net = net_rows[-1] if net_rows else None
    hash_series = [(int(r["unix_time"]), float(r["hashes_per_second"])) for r in net_rows
                   if int(r["unix_time"]) >= t_now - 86400 and r.get("hashes_per_second")]

    agree_rows = tail_rows(os.path.join(a.monitor_dir, "agreement.csv"), 300_000)
    agree = {r["node"]: r for r in agree_rows}

    # recent blocks and the last 24 h
    recent = deque(tail_rows(os.path.join(a.monitor_dir, "blocks.csv"), 4_000_000), maxlen=200_000)
    day_ago = t_now - 86400
    last24_rows = [b for b in recent if int(b["unix_time"]) >= day_ago]
    last24 = Counter(b["kind"] for b in last24_rows)
    hour_rows = [b for b in recent if int(b["unix_time"]) >= t_now - 3600]
    last_hour = Counter(b["kind"] for b in hour_rows)
    payouts24 = len({b["payout_script"] for b in last24_rows if b.get("payout_script")})
    tags24 = Counter(tag(b["extra_data"]) or "(none)" for b in last24_rows)
    recent_list = sorted(recent, key=lambda b: (int(b["daa_score"]), b["hash"]), reverse=True)[:RECENT]

    if daa >= a.activation:
        phase, phase_key = "Merged mining active", "merged"
    elif daa >= ban_start:
        phase, phase_key = "Native phase · self-parent ban active", "ban"
    else:
        phase, phase_key = "Native phase", "native"

    def eta(target):
        if daa >= target:
            return {"reached": True, "unix": st["reached"].get("ban" if target == ban_start else "activation")}
        if not rate or rate <= 0:
            return {"reached": False, "unix": None}
        return {"reached": False, "unix": int(t_now + (target - daa) / rate)}

    nodes = []
    for n in order:
        r = latest[n]
        ag = agree.get(n)
        miss = st["first_miss"].get(n)
        nodes.append({
            "node": n,
            "role": "reference" if n == ref else ("old node (no merged mining)" if n in a.old else "upgraded"),
            "old": n in a.old,
            "reachable": r["status"] == "ok",
            "synced": r.get("is_synced") == "1",
            "virtual_daa_score": int(r["virtual_daa_score"]) if r["status"] == "ok" and r["virtual_daa_score"] else None,
            "peers": int(r["peers"]) if r["status"] == "ok" and r["peers"] else None,
            "version": r.get("server_version", "") if r["status"] == "ok" else "",
            "seen": int(r["unix_time"]),
            "has_reference_chain": None if n == ref or not ag else (ag["present"] == "1" and ag["is_chain_block"] == "1"),
            "first_miss_daa": miss["daa_score"] if miss else None,
        })

    days = sorted(st["days"].items())[-30:]

    # ZKas (monitor --zkas): absent unless the monitor follows a ZKas node
    zkas = None
    zkas_nodes = tail_rows(os.path.join(a.monitor_dir, "zkas.csv"), 200_000)
    if zkas_nodes:
        zr = zkas_nodes[-1]
        zrecent = tail_rows(os.path.join(a.monitor_dir, "zkas_blocks.csv"), 2_000_000)
        zkas = {
            "node": zr["node"],
            "reachable": zr["status"] == "ok",
            "synced": zr.get("is_synced") == "1",
            "virtual_daa_score": int(zr["virtual_daa_score"]) if zr["status"] == "ok" and zr["virtual_daa_score"] else None,
            "version": zr.get("server_version", "") if zr["status"] == "ok" else "",
            "seen": int(zr["unix_time"]),
            "blocks_all_time": st["zkas_kinds"],
            "blocks_last_24h": dict(Counter(b["kind"] for b in zrecent if int(b["unix_time"]) >= day_ago)),
            "blocks_per_day": dict(sorted(st["zkas_days"].items())[-30:]),
        }
    shared_recent = tail_rows(os.path.join(a.monitor_dir, "shared_parents.csv"), 100_000)[-8:]
    status = {
        "network": "sedra-testnet-10",
        "test_network": True,
        "generated": t_now,
        "data_time": data_time,
        "stale": t_now - data_time > STALE_SECONDS,
        "virtual_daa_score": daa,
        "daa_per_second_last_hour": rate,
        "difficulty": float(net["difficulty"]) if net and net.get("difficulty") else None,
        "hashes_per_second": float(net["hashes_per_second"]) if net and net.get("hashes_per_second") else None,
        "phase": phase,
        "ban_start": ban_start,
        "activation": a.activation,
        "ban_start_eta": eta(ban_start),
        "activation_eta": eta(a.activation),
        "blocks_all_time": st["kinds"],
        "blocks_last_24h": dict(last24),
        "blocks_last_hour": dict(last_hour),
        "blocks_since_activation": st["after_activation"],
        "blocks_per_day": dict(days),
        "share_per_tag": st["tags"],
        "tags_last_24h": dict(tags24),
        "distinct_payout_scripts_last_24h": payouts24,
        "first_merged_block": st["first_merged"],
        "sdm1_blocks_after_ban_start": [b for b in st["sdm1"] if b["daa_score"] >= ban_start],
        "nodes": nodes,
        "recent_blocks": [{k: b[k] for k in ("hash", "daa_score", "unix_time", "kind", "is_chain_block", "red")}
                          | {"tag": tag(b["extra_data"])} for b in recent_list],
        "seeds": a.seed,
        "pool": a.pool,
        "zkas": zkas,
        "shared_parents": {"count": st["shared_count"], "first": st["shared_first"],
                           "recent": [{k: r[k] for k in ("unix_time", "parent_prefix", "sedra_hash", "zkas_hash")}
                                      for r in reversed(shared_recent)]},
    }
    status["criteria"] = criteria_status(status, st, a, load_criteria(a.criteria))

    ver = copy_assets(a.out)
    write_atomic(os.path.join(a.out, "status.json"), json.dumps(status, indent=1))
    write_atomic(os.path.join(a.out, "index.html"), render(status, a, phase_key, hash_series, ver))
    write_atomic(state_path, json.dumps(st))
    if a.publish:
        publish(a.out, a.publish)


def copy_assets(out):
    """Copies status_assets/ next to this script into the output; returns a short content hash for the ?v= query, so
    browsers fetch a changed stylesheet at once."""
    src = os.path.join(os.path.dirname(os.path.abspath(__file__)), ASSETS)
    dst = os.path.join(out, ASSETS)
    os.makedirs(dst, exist_ok=True)
    h = hashlib.sha256()
    for name in sorted(os.listdir(src)):
        if name.endswith(".example.json"):
            continue
        p = os.path.join(src, name)
        with open(p, "rb") as f:
            data = f.read()
        h.update(name.encode() + data)
        q = os.path.join(dst, name)
        try:
            with open(q, "rb") as f:
                same = f.read() == data
        except OSError:
            same = False
        if not same:
            shutil.copyfile(p, q)
            os.chmod(q, 0o644)
    return h.hexdigest()[:10]


def publish(out, target):
    """Copies the page to the public web host in one rsync call. The key used should be restricted on that host to
    this one directory (rrsync -wo), see contrib/testnet/README.md. --delay-updates puts all changed files in place at
    the end, so a visitor never gets a page that points at an asset version that is not there yet.

    One SSH connection per minute from the home network can trip the router's intrusion prevention (seen 2026-11-24:
    port 22 to the VPS blocked after about eight connections in a few minutes), so the unit sends rsync over a
    persistent SSH master connection (systemd/sedra-testnet-link.service). With $STATUS_PUSH_SOCKET set, the page
    is not pushed while that master socket is missing, instead of opening a new connection every minute."""
    sock = os.environ.get("STATUS_PUSH_SOCKET")
    if sock and (not os.path.exists(sock) or subprocess.run(
            ["ssh", "-S", sock, "-O", "check", "vps"], capture_output=True).returncode != 0):
        print(f"not published: no live SSH master on {sock} (sedra-testnet-link.service down?)")
        return
    files = [os.path.join(out, ASSETS), os.path.join(out, "status.json"), os.path.join(out, "index.html")]
    subprocess.run(["rsync", "-rt", "--delay-updates", "--chmod=D755,F644", "--timeout=30", *files, target], check=True)


def write_atomic(path, text):
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".tmp-")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)
    os.chmod(tmp, 0o644)
    os.replace(tmp, path)


def svg_days(days):
    """Stacked bars per UTC day, native below merged-mined. Server-side SVG: no script needed."""
    if not days:
        return '<p class="muted">No blocks recorded yet.</p>'
    w, h, gap = 640, 150, 4
    n = len(days)
    bw = max(6, (w - gap * (n - 1)) / n)
    top = max(1, max(sum(v.values()) for _, v in days))
    bars = []
    for i, (d, v) in enumerate(days):
        x = i * (bw + gap)
        nat, mer = v.get("native", 0), v.get("merged", 0)
        hn, hm = h * nat / top, h * mer / top
        label = html.escape(f"{d}: {nat:,} native, {mer:,} merged-mined")
        bars.append(f'<g><title>{label}</title><rect class="b-native" x="{x:.1f}" y="{h - hn:.1f}" width="{bw:.1f}" height="{hn:.1f}" rx="2"/>'
                    f'<rect class="b-merged" x="{x:.1f}" y="{h - hn - hm:.1f}" width="{bw:.1f}" height="{hm:.1f}" rx="2"/></g>')
    first, last = days[0][0], days[-1][0]
    return (f'<svg class="chart" viewBox="0 -20 {w} {h + 40}" role="img" aria-label="Blocks per day, native and merged-mined">'
            f'{"".join(bars)}<line class="axis" x1="0" y1="{h}" x2="{w}" y2="{h}"/>'
            f'<text x="0" y="{h + 18}">{first}</text><text x="{w}" y="{h + 18}" text-anchor="end">{last}</text>'
            f'<text x="{w}" y="-8" text-anchor="end">max {top:,}/day</text></svg>')


def svg_line(series):
    """Hashrate estimate over the last 24 h as a line."""
    if len(series) < 2:
        return '<p class="muted">Not enough samples yet.</p>'
    w, h = 640, 120
    t0, t1 = series[0][0], series[-1][0]
    top = max(v for _, v in series) or 1
    pts = " ".join(f"{(t - t0) / max(1, t1 - t0) * w:.1f},{h - v / top * (h - 6):.1f}" for t, v in series)
    return (f'<svg class="chart" viewBox="0 -20 {w} {h + 40}" role="img" aria-label="Hashrate estimate, last 24 hours">'
            f'<polyline class="line" points="{pts}"/><line class="axis" x1="0" y1="{h}" x2="{w}" y2="{h}"/>'
            f'<text x="0" y="{h + 18}">{utc(t0)}</text><text x="{w}" y="{h + 18}" text-anchor="end">{utc(t1)}</text>'
            f'<text x="{w}" y="-8" text-anchor="end">max {si(top, "H/s")}</text></svg>')


def coin_tree(v, zkas):
    """The hero's coin group: Kaspa on top, arrows down to Sedra (and ZKas when the monitor follows it). You mine Kaspa;
    the merged-mined coins come on top of it. One inline SVG, so it scales as a unit and the arrows take the theme's
    colour (currentColor)."""
    label = "Mine KAS, get SDR and/or ZKAS on top" if zkas else "Mine KAS, get SDR on top"
    if zkas:
        lows = (f'<image href="{ASSETS}/sedra.svg{v}" x="0" y="64" width="44" height="44"/>'
                f'<image href="{ASSETS}/zkas.svg{v}" x="88" y="64" width="44" height="44"/>'
                '<path d="M54 44 L30 64"/><path d="M78 44 L102 64"/>')
        caption = "mine KAS · get SDR and/or ZKAS"
    else:
        lows = f'<image href="{ASSETS}/sedra.svg{v}" x="44" y="64" width="44" height="44"/><path d="M66 46 L66 62"/>'
        caption = "mine KAS · get SDR on top"
    return (f'<figure class="hero-coins" title="{label}"><svg viewBox="0 0 132 108" role="img" aria-label="{label}">'
            '<defs><marker id="tip" viewBox="0 0 6 6" refX="5" refY="3" markerWidth="5" markerHeight="5" orient="auto">'
            '<path d="M0 0 L6 3 L0 6 z" fill="currentColor" stroke="none"/></marker></defs>'
            f'<image href="{ASSETS}/kaspa.svg{v}" x="44" y="0" width="44" height="44"/>'
            f'<g class="branch" marker-end="url(#tip)">{lows}</g></svg>'
            f'<figcaption>{caption}</figcaption></figure>')


def render(s, a, phase_key, hash_series, ver):
    e = html.escape
    links = {k: getattr(a, k) or v for k, v in DEFAULTS.items()}

    def yesno(v, good="yes", bad="no"):
        if v is None:
            return '<span class="muted">-</span>'
        return f'<span class="pill ok">{good}</span>' if v else f'<span class="pill bad">{bad}</span>'

    def countdown(label, target, info, key):
        pct = min(100.0, 100.0 * s["virtual_daa_score"] / target) if target else 100.0
        if info["reached"]:
            when = f"reached {utc(info['unix'])}" if info["unix"] else "reached"
            left, cls = "reached", "done"
        elif info["unix"]:
            when = f"about {utc(info['unix'])}"
            left, cls = f"in about {span(info['unix'] - s['generated'])}", ""
        else:
            when, left, cls = "date unknown until the block rate is measured", "-", ""
        unix = info["unix"] if info["unix"] and not info["reached"] else ""
        remaining = max(0, target - s["virtual_daa_score"])
        return (f'<div class="card countdown {cls}"><div class="card-label">{label}</div>'
                f'<div class="big" data-eta="{unix}" id="cd-{key}">{e(left)}</div>'
                f'<div class="bar"><i style="width:{pct:.1f}%"></i></div>'
                f'<div class="muted small">DAA {target:,} · {remaining:,} to go · {e(when)}</div></div>')

    ok_nodes = sum(1 for n in s["nodes"] if n["reachable"])
    peers = [n["peers"] for n in s["nodes"] if n["peers"] is not None]
    rows_nodes = "".join(
        f"<tr><td><b>{e(n['node'])}</b></td><td>{e(n['role'])}</td><td>{yesno(n['reachable'])}</td><td>{yesno(n['synced'])}</td>"
        f"<td class=num>{f'{n['virtual_daa_score']:,}' if n['virtual_daa_score'] is not None else '-'}</td>"
        f"<td class=num>{n['peers'] if n['peers'] is not None else '-'}</td><td class=mono>{e(n['version']) or '-'}</td>"
        f"<td>{'<span class=muted>reference</span>' if n['role'] == 'reference' else yesno(n['has_reference_chain'])}</td>"
        f"<td class=num>{f'{n['first_miss_daa']:,}' if n['first_miss_daa'] is not None else '<span class=muted>none</span>'}</td></tr>"
        for n in s["nodes"]) or '<tr><td colspan="9" class="muted">No node data yet.</td></tr>'
    rows_blocks = "".join(
        f"<tr><td>{utc_s(b['unix_time'])[11:]}</td><td class=num>{int(b['daa_score']):,}</td><td class=mono>{e(b['hash'][:16])}…</td>"
        f"<td><span class=\"kind {e(b['kind'])}\">{'merged-mined' if b['kind'] == 'merged' else e(b['kind'])}</span></td>"
        f"<td>{e(b['tag'] or '-')}</td><td>{'<span class=\"pill bad\">red</span>' if b['red'] == '1' else ''}</td></tr>"
        for b in s["recent_blocks"]) or '<tr><td colspan="6" class="muted">No blocks yet.</td></tr>'
    tags = sorted(s["share_per_tag"].items(), key=lambda kv: -kv[1])[:12]
    total = max(1, sum(s["blocks_all_time"].values()))
    rows_tags = "".join(
        f"<tr><td>{e(k.split('|')[0])}</td><td><span class=\"kind {e(k.split('|')[1])}\">{'merged-mined' if k.split('|')[1] == 'merged' else e(k.split('|')[1])}</span></td>"
        f"<td class=num>{v:,}</td><td class=num>{100 * v / total:.1f}%</td>"
        f"<td class=num>{s['tags_last_24h'].get(k.split('|')[0], 0):,}</td></tr>" for k, v in tags) \
        or '<tr><td colspan="5" class="muted">No blocks yet.</td></tr>'
    rows_crit = "".join(
        f'<li class="crit {STATES[c["status"]][1]}"><span class="crit-n">{c["n"]}</span><div><div class="crit-text">{e(c["text"])}</div>'
        f'<div class="crit-note">{e(c["note"])}'
        f'{f" · <a href=\"{e(c["link"])}\">evidence</a>" if c["link"] else ""}</div></div>'
        f'<span class="pill {STATES[c["status"]][1]}">{STATES[c["status"]][0]}</span></li>' for c in s["criteria"])
    first = s["first_merged_block"]
    first_txt = (f'DAA {first["daa_score"]:,}<span class="muted small mono">{e(first["hash"][:16])}… · {utc(first["unix_time"])}</span>'
                 if first else 'none yet<span class="muted small">expected at the activation</span>')
    late_sdm1 = len(s["sdm1_blocks_after_ban_start"])
    stale = ('<div class="alert" id="stale">The monitor data is older than five minutes: this page may be out of date.</div>'
             if s["stale"] else '<div class="alert" id="stale" hidden></div>')
    rate = f"{s['daa_per_second_last_hour']:.2f}" if s["daa_per_second_last_hour"] else "-"
    seeds = "".join(f"<li><code>{e(x)}</code></li>" for x in s["seeds"]) or "<li class=muted>announced at launch</li>"
    pool = f"<code>{e(s['pool'])}</code>" if s["pool"] else '<span class="muted">announced before the activation</span>'
    report = f'<a href="{e(a.report_url)}">Daily reports</a>' if a.report_url else ""
    h24 = s["blocks_last_24h"]
    h1 = s["blocks_last_hour"]
    merged_share_1h = 100 * h1.get("merged", 0) / max(1, sum(h1.values()))
    v = f"?v={ver}"
    z = s["zkas"]
    sp = s["shared_parents"]
    coins = coin_tree(v, bool(z))
    zkas_html = ""
    if z:
        zall, z24 = z["blocks_all_time"], z["blocks_last_24h"]
        rows_pairs = "".join(
            f"<tr><td>{utc_s(r['unix_time'])[5:]}</td><td class=mono>{e(r['parent_prefix'])}…</td>"
            f"<td class=mono>{e(r['sedra_hash'][:16])}…</td><td class=mono>{e(r['zkas_hash'][:16])}…</td></tr>"
            for r in sp["recent"]) or '<tr><td colspan="4" class="muted">None yet: Sedra blocks are merged-mined from the activation on.</td></tr>'
        ztip = f"{z['virtual_daa_score']:,}" if z["virtual_daa_score"] is not None else "-"
        zkas_html = f"""
<h3 id="zkas">ZKas test chain under the same Kaspa parents</h3>
<p class="muted small">The testnet pool also merges a private ZKas test chain (pool only, not a public network), as the mainnet pool
would. A Sedra block and a ZKas block share a Kaspa parent when their nonces match: on both chains a merged-mined block's nonce
is the first 8 bytes of its parent's hash.</p>
<div class="grid4 inner">
<div class="card"><div class="card-label"><span class="sw merged"></span>ZKas merged-mined</div><div class="big">{zall.get('merged', 0):,}</div><div class="muted small">last 24 h: {z24.get('merged', 0):,} · native {zall.get('native', 0):,}</div></div>
<div class="card"><div class="card-label">Shared Kaspa parents</div><div class="big {'good' if sp['count'] else ''}">{sp['count']:,}</div><div class="muted small">one parent, a Sedra and a ZKas block</div></div>
<div class="card"><div class="card-label">ZKas tip (DAA)</div><div class="big">{ztip}</div><div class="muted small">{'reachable' if z['reachable'] else 'unreachable'} · {'synced' if z['synced'] else 'not synced'}</div></div>
<div class="card"><div class="card-label">ZKas node</div><div class="big medium">{e(z['node'])}<span class="muted small mono">{e(z['version']) or '-'}</span></div></div>
</div>
<div class="chart-grid">
<div><h3>ZKas blocks per day</h3><div class="legend"><span><i class="sw native"></i>native</span><span><i class="sw merged"></i>merged-mined</span></div>{svg_days(list(z['blocks_per_day'].items()))}</div>
<div><h3>Newest shared parents</h3><div class="scroll"><table><thead><tr><th>seen (UTC)</th><th>Kaspa parent</th><th>Sedra block</th><th>ZKas block</th></tr></thead><tbody>{rows_pairs}</tbody></table></div></div>
</div>"""
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="60">
<title>Sedra Testnet Status · TEST NETWORK</title>
<meta name="description" content="Live status of the Sedra merged-mining public testnet (sedra-testnet-10). Test coins only, no value.">
<meta name="robots" content="index, nofollow">
<link rel="icon" type="image/svg+xml" href="{ASSETS}/logo.svg{v}">
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Fira+Code:wght@400;500&family=Inter:wght@400;500;600;700&family=Orbitron:wght@700;800&family=Space+Grotesk:wght@500;600;700&display=swap" rel="stylesheet">
<link rel="stylesheet" href="{ASSETS}/status.css{v}">
<script>try{{var t=localStorage.getItem("theme");if(t)document.documentElement.setAttribute("data-theme",t)}}catch(e){{}}</script>
</head><body class="phase-{phase_key}" data-generated="{s['generated']}" data-data-time="{s['data_time']}">
<header class="top">
<div class="testnet-banner" role="note"><b>TESTNET</b> — test coins, no value.<span class="wide"> Nothing here concerns Sedra mainnet.</span></div>
<div class="top-inner">
<a class="brand" href="{e(links['home_url'])}" title="Back to puddlepool.com"><span class="halo"><img src="{ASSETS}/logo.svg{v}" alt=""></span>
<span class="brand-text"><span class="brand-name">puddlepool.com</span><span class="brand-badge">Sedra testnet status</span></span></a>
<nav class="top-nav"><a href="#join">Join</a><a href="#criteria">Pass criteria</a><a href="#blocks">Blocks</a><a href="#nodes">Nodes</a>
<a href="{e(links['home_url'])}">← puddlepool.com</a>
<button type="button" class="theme-btn" id="theme-btn" hidden aria-label="Switch light or dark theme">◐</button></nav>
</div></header>
<main>
<section class="hero">
{coins}
<div><h1>Sedra merged-mining testnet</h1>
<p class="lead">A public test of <a href="{e(links['sip_url'])}">SIP-0001 merged mining</a> on <code>sedra-testnet-10</code>,
with Kaspa testnet-10 as the parent chain. Native CPU blocks first, then merged-mined blocks next to them.</p>
<p class="muted small">Updated <span id="updated">{utc_s(s['generated'])}</span> · monitor data from {utc_s(s['data_time']) if s['data_time'] else 'never'}
· refreshes every minute · <a href="status.json">status.json</a>{' · ' + report if report else ''}</p></div>
<div class="phase-chip"><span class="dot"></span>{e(s['phase'])}</div>
</section>
{stale}
<section class="grid4">
<div class="card"><div class="card-label">Virtual DAA score</div><div class="big">{s['virtual_daa_score']:,}</div><div class="muted small">{rate} DAA/s over the last hour</div></div>
<div class="card"><div class="card-label">Difficulty</div><div class="big">{si(s['difficulty'])}</div><div class="muted small">reference node</div></div>
<div class="card"><div class="card-label">Hashrate estimate</div><div class="big">{si(s['hashes_per_second'], 'H/s')}</div><div class="muted small">native + merged, last 1,000 blocks</div></div>
<div class="card"><div class="card-label">Nodes up</div><div class="big">{ok_nodes} / {len(s['nodes'])}</div><div class="muted small">peers per node: {', '.join(map(str, peers)) or '-'}</div></div>
</section>
<section class="grid2">
{countdown("Self-parent ban starts", s['ban_start'], s['ban_start_eta'], "ban")}
{countdown("Merged mining activates", s['activation'], s['activation_eta'], "act")}
</section>

<section id="join" class="panel"><h2>Join the testnet</h2>
<div class="join-grid">
<a class="join-card" href="{e(links['guide_url'])}"><b>Participant guide</b><span>Run a node, make a wallet, mine natively or merged</span></a>
<a class="join-card" href="{e(links['faucet_url'])}"><b>Faucet</b><span>No miner? Open a "Testnet report" issue, type faucet, with your <code>sedratest:</code> address</span></a>
<a class="join-card" href="{e(links['release_url'])}"><b>Release 0.2.0-testnet.1</b><span>Signed tag, Linux binaries; refuses mainnet</span></a>
<a class="join-card" href="{e(links['checksums_url'])}"><b>Checksums</b><span>SHA256SUMS; verify before you run it (guide §3)</span></a>
</div>
<div class="join-cols"><div><h3>Seed nodes</h3><p class="muted small">Add with <code>--addpeer</code></p><ul class="plain">{seeds}</ul></div>
<div><h3>Merged-mining pool</h3><p>{pool}</p><p class="muted small">Kaspa testnet-10 stratum; your Sedra testnet address goes in the login (guide §7).</p></div></div>
</section>

<section id="criteria" class="panel"><h2>SIP-0003 pass criteria</h2>
<p class="muted small">Criteria 1, 2, 3 and 8 and the shared parents of 9 are read live from the monitor; 4 to 7 and the rest of 9 are recorded by the operator with evidence.
The final verdict is the dated public report, not this page.</p>
<ol class="crit-list">{rows_crit}</ol></section>

<section id="blocks" class="panel"><h2>Blocks</h2>
<div class="grid4 inner">
<div class="card"><div class="card-label"><span class="sw native"></span>Native</div><div class="big">{s['blocks_all_time'].get('native', 0):,}</div><div class="muted small">last 24 h: {h24.get('native', 0):,}</div></div>
<div class="card"><div class="card-label"><span class="sw merged"></span>Merged-mined</div><div class="big">{s['blocks_all_time'].get('merged', 0):,}</div><div class="muted small">last 24 h: {h24.get('merged', 0):,} · last hour {merged_share_1h:.0f}%</div></div>
<div class="card"><div class="card-label">First merged-mined block</div><div class="big medium">{first_txt}</div></div>
<div class="card"><div class="card-label">SDM1 coinbases after the ban start</div><div class="big {'bad' if late_sdm1 else 'good'}">{late_sdm1}</div><div class="muted small">must stay 0</div></div>
</div>
<div class="chart-grid">
<div><h3>Blocks per day</h3><div class="legend"><span><i class="sw native"></i>native</span><span><i class="sw merged"></i>merged-mined</span></div>{svg_days(list(s['blocks_per_day'].items()))}</div>
<div><h3>Hashrate estimate, 24 h</h3><div class="legend"><span><i class="sw line"></i>H/s (reference node)</span></div>{svg_line(hash_series)}</div>
</div>
<h3>Miners and pools seen (coinbase tag)</h3>
<p class="muted small">{s['distinct_payout_scripts_last_24h']:,} distinct payout addresses in the last 24 h (not listed).</p>
<div class="scroll"><table><thead><tr><th>tag</th><th>kind</th><th class=num>blocks</th><th class=num>share</th><th class=num>last 24 h</th></tr></thead><tbody>{rows_tags}</tbody></table></div>
{zkas_html}
</section>

<section id="nodes" class="panel"><h2>Nodes</h2>
<p class="muted small">"Has the chain": the node has the newest reference chain block that is at least 100 DAA deep, on its selected chain.
Upgraded nodes must always say yes; the old node is expected to say no from the first merged-mined block on.</p>
<div class="scroll"><table><thead><tr><th>node</th><th>role</th><th>reachable</th><th>synced</th><th class=num>DAA</th><th class=num>peers</th><th>version</th><th>has the chain</th><th class=num>first miss (DAA)</th></tr></thead><tbody>{rows_nodes}</tbody></table></div>
</section>

<section class="panel"><h2>Recent blocks</h2>
<p class="muted small">native = its own proof of work meets the Sedra target; merged-mined = accepted through a Kaspa testnet-10 parent (SIP-0001). Times in UTC.</p>
<div class="scroll"><table><thead><tr><th>recorded</th><th class=num>DAA</th><th>hash</th><th>kind</th><th>coinbase tag</th><th></th></tr></thead><tbody>{rows_blocks}</tbody></table></div>
</section>
</main>
<footer class="foot"><p><b>TEST NETWORK.</b> Testnet coins have no value and are never exchanged. This page is not an explorer and says nothing about a mainnet date.</p>
<p><a href="{e(links['home_url'])}">puddlepool.com</a> · <a href="{GITHUB}/sedra-project/tree/main/testnet">testnet docs</a> · <a href="{GITHUB}/sedra-rusty">sedra-rusty</a> · operated by Nec</p></footer>
<script src="{ASSETS}/status.js{v}" defer></script>
</body></html>
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("monitor_dir")
    ap.add_argument("--out", required=True, help="directory the web server serves (or the staging directory for --publish)")
    ap.add_argument("--activation", type=int, default=345_600, help="merged-mining activation DAA (release 0.2.0-testnet.1: 345600)")
    ap.add_argument("--finality", type=int, default=86_400, help="finality depth (testnet: 86400)")
    ap.add_argument("--old", action="append", default=[], help="name of a node without merged mining (repeatable)")
    ap.add_argument("--seed", action="append", default=[], help="public seed address to list (repeatable)")
    ap.add_argument("--pool", default="", help="public stratum address of the testnet pool")
    ap.add_argument("--report-url", default="", help="link to the reports (for example issue #13)")
    ap.add_argument("--criteria", default="", help="operator JSON for criteria 4-7 (see status_assets/criteria.example.json)")
    ap.add_argument("--publish", default="", help="rsync target, e.g. testnetlink@host:/ (a restricted key, see README)")
    for k, v in DEFAULTS.items():
        ap.add_argument("--" + k.replace("_", "-"), dest=k, default="", help=f"link (default {v})")
    build(ap.parse_args())


if __name__ == "__main__":
    main()
