#!/usr/bin/env python3
"""SEDRA (#13): public status page of the Sedra testnet, built from the CSV files of sedra-testnet-monitor.

SIP-0003 section 4.4 promises "a status page with recent blocks". This script writes a static `index.html` and a
`status.json` into an output directory that any web server can serve. It runs once per call (a systemd timer calls it
every minute, see systemd/sedra-testnet-status.timer); it needs only the Python standard library and never talks to a
node itself, so the page can sit on a host without RPC access.

Usage:
  python3 status_page.py MONITOR_DIR --out WWW_DIR [--activation 345600] [--finality 86400]
         [--old old-go] [--seed host:22211 ...] [--pool stratum+tcp://host:15555] [--report-url URL]

Shown: the phase (native, ban, merged mining) with progress to the ban start and the activation; every node with its
state; whether each node still has the reference chain; block counts (native and merged-mined, all time and last 24 h)
with the share per coinbase tag; the most recent blocks. Not shown on purpose: payout scripts, node errors (they can
contain addresses) and anything about hosts beyond the seed addresses given on the command line.

blocks.csv grows by about 86,400 rows a day, so it is read incrementally: a small state file in the output directory
keeps the byte offset and the counters. Delete it to recount from the start.
"""
import argparse
import csv
import datetime as dt
import html
import io
import json
import os
import tempfile
from collections import Counter, deque

STATE = ".status-state.json"
RECENT = 40
STALE_SECONDS = 300


def now():
    return int(dt.datetime.now(dt.timezone.utc).timestamp())


def utc(t):
    return dt.datetime.fromtimestamp(int(t), dt.timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


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


def load_state(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {"offset": 0, "header": None, "kinds": {}, "tags": {}, "days": {}, "first_merged": None, "sdm1": []}


def read_new_blocks(path, st):
    """Adds the rows written since the last call to the counters; returns the new rows."""
    if not os.path.exists(path):
        return []
    with open(path, "rb") as f:
        size = os.fstat(f.fileno()).st_size
        if size < st["offset"]:  # file replaced (reset): start over
            st.update(load_state("/nonexistent"))
        if st["offset"] == 0:
            st["header"] = f.readline().decode().strip()
            st["offset"] = f.tell()
        f.seek(st["offset"])
        data = f.read()
    end = data.rfind(b"\n") + 1  # only complete lines; the monitor may be writing the last one
    st["offset"] += end
    new = list(csv.DictReader(io.StringIO(st["header"] + "\n" + data[:end].decode(errors="replace"))))
    for b in new:
        k = b["kind"]
        st["kinds"][k] = st["kinds"].get(k, 0) + 1
        key = f"{tag(b['extra_data']) or '(none)'}|{k}"
        st["tags"][key] = st["tags"].get(key, 0) + 1
        d = dt.datetime.fromtimestamp(int(b["unix_time"]), dt.timezone.utc).strftime("%Y-%m-%d")
        st["days"].setdefault(d, {})
        st["days"][d][k] = st["days"][d].get(k, 0) + 1
        if k == "merged" and (st["first_merged"] is None or int(b["daa_score"]) < st["first_merged"]["daa_score"]):
            st["first_merged"] = {"hash": b["hash"], "daa_score": int(b["daa_score"]), "unix_time": int(b["unix_time"])}
        if "SDM1" in b["extra_data"]:
            st["sdm1"] = (st["sdm1"] + [{"hash": b["hash"], "daa_score": int(b["daa_score"])}])[-20:]
    return new


def build(a):
    os.makedirs(a.out, exist_ok=True)
    state_path = os.path.join(a.out, STATE)
    st = load_state(state_path)
    read_new_blocks(os.path.join(a.monitor_dir, "blocks.csv"), st)
    ban_start = max(0, a.activation - 2 * a.finality)
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

    # agreement: the newest check per node, and the first miss ever seen in the tail
    agree_rows = tail_rows(os.path.join(a.monitor_dir, "agreement.csv"))
    agree, first_miss = {}, {}
    for r in agree_rows:
        agree[r["node"]] = r
        if (r["present"] == "0" or r["is_chain_block"] == "0") and r["node"] not in first_miss:
            first_miss[r["node"]] = r

    # recent blocks and the last 24 h
    recent = deque(tail_rows(os.path.join(a.monitor_dir, "blocks.csv"), 4_000_000), maxlen=200_000)
    day_ago = t_now - 86400
    last24 = Counter(b["kind"] for b in recent if int(b["unix_time"]) >= day_ago)
    recent_list = sorted(recent, key=lambda b: (int(b["daa_score"]), b["hash"]), reverse=True)[:RECENT]

    if daa >= a.activation:
        phase = "merged mining active"
    elif daa >= ban_start:
        phase = "native phase, self-parent ban active"
    else:
        phase = "native phase"

    def eta(target):
        if daa >= target:
            return "reached"
        if not rate or rate <= 0:
            return "unknown"
        return "about " + utc(t_now + (target - daa) / rate)

    nodes = []
    for n in order:
        r = latest[n]
        ag = agree.get(n)
        nodes.append({
            "node": n,
            "role": "reference" if n == ref else ("old node, expected to stop at the first merged-mined block" if n in a.old else "upgraded"),
            "reachable": r["status"] == "ok",
            "synced": r.get("is_synced") == "1",
            "virtual_daa_score": int(r["virtual_daa_score"]) if r["status"] == "ok" and r["virtual_daa_score"] else None,
            "peers": int(r["peers"]) if r["status"] == "ok" and r["peers"] else None,
            "version": r.get("server_version", "") if r["status"] == "ok" else "",
            "seen": int(r["unix_time"]),
            "has_reference_chain": None if not ag else (ag["present"] == "1" and ag["is_chain_block"] == "1"),
            "first_miss_daa": int(first_miss[n]["daa_score"]) if n in first_miss else None,
        })

    status = {
        "network": "sedra-testnet-10",
        "generated": t_now,
        "data_time": data_time,
        "stale": t_now - data_time > STALE_SECONDS,
        "virtual_daa_score": daa,
        "daa_per_second_last_hour": rate,
        "phase": phase,
        "ban_start": ban_start,
        "activation": a.activation,
        "ban_start_eta": eta(ban_start),
        "activation_eta": eta(a.activation),
        "blocks_all_time": st["kinds"],
        "blocks_last_24h": dict(last24),
        "blocks_per_day": st["days"],
        "share_per_tag": st["tags"],
        "first_merged_block": st["first_merged"],
        "sdm1_blocks_after_ban_start": [b for b in st["sdm1"] if b["daa_score"] >= ban_start],
        "nodes": nodes,
        "recent_blocks": [{k: b[k] for k in ("hash", "daa_score", "unix_time", "kind", "is_chain_block", "red")}
                          | {"tag": tag(b["extra_data"])} for b in recent_list],
        "seeds": a.seed,
        "pool": a.pool,
    }

    write_atomic(os.path.join(a.out, "status.json"), json.dumps(status, indent=1))
    write_atomic(os.path.join(a.out, "index.html"), render(status, a))
    write_atomic(state_path, json.dumps(st))


def write_atomic(path, text):
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".tmp-")
    with os.fdopen(fd, "w") as f:
        f.write(text)
    os.chmod(tmp, 0o644)
    os.replace(tmp, path)


def render(s, a):
    e = html.escape
    pct = lambda target: min(100.0, 100.0 * s["virtual_daa_score"] / target) if target else 100.0

    def yesno(v, good="yes", bad="no"):
        if v is None:
            return '<span class="muted">-</span>'
        return f'<span class="ok">{good}</span>' if v else f'<span class="bad">{bad}</span>'

    rows_nodes = "".join(
        f"<tr><td>{e(n['node'])}</td><td>{e(n['role'])}</td><td>{yesno(n['reachable'])}</td><td>{yesno(n['synced'])}</td>"
        f"<td class=num>{n['virtual_daa_score'] if n['virtual_daa_score'] is not None else '-'}</td>"
        f"<td class=num>{n['peers'] if n['peers'] is not None else '-'}</td><td>{e(n['version'])}</td>"
        f"<td>{yesno(n['has_reference_chain'])}</td>"
        f"<td class=num>{n['first_miss_daa'] if n['first_miss_daa'] is not None else 'none'}</td></tr>"
        for n in s["nodes"])
    rows_blocks = "".join(
        f"<tr><td>{utc(b['unix_time'])}</td><td class=num>{b['daa_score']}</td><td><code>{e(b['hash'][:16])}…</code></td>"
        f"<td><span class=\"kind {e(b['kind'])}\">{e(b['kind'])}</span></td><td>{e(b['tag'] or '-')}</td>"
        f"<td>{'red' if b['red'] == '1' else ''}</td></tr>"
        for b in s["recent_blocks"])
    tags = sorted(s["share_per_tag"].items(), key=lambda kv: -kv[1])[:12]
    total = max(1, sum(s["blocks_all_time"].values()))
    rows_tags = "".join(
        f"<tr><td>{e(k.split('|')[0])}</td><td>{e(k.split('|')[1])}</td><td class=num>{v}</td>"
        f"<td class=num>{100 * v / total:.1f}%</td></tr>" for k, v in tags)
    first = s["first_merged_block"]
    first_txt = (f"DAA {first['daa_score']}, <code>{e(first['hash'][:16])}…</code>, {utc(first['unix_time'])}"
                 if first else "none yet")
    late_sdm1 = len(s["sdm1_blocks_after_ban_start"])
    stale = ('<p class="warn">The monitor data is older than five minutes: the page may be out of date.</p>'
             if s["stale"] else "")
    rate = f"{s['daa_per_second_last_hour']:.2f}" if s["daa_per_second_last_hour"] else "unknown"
    seeds = "".join(f"<li><code>{e(x)}</code></li>" for x in s["seeds"]) or "<li>announced at launch</li>"
    pool = f"<code>{e(s['pool'])}</code>" if s["pool"] else "announced at launch"
    report = f' · <a href="{e(a.report_url)}">reports and daily numbers</a>' if a.report_url else ""
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="60"><title>Sedra Testnet Status</title>
<style>
:root{{--bg:#fbfbfa;--fg:#1d1d1b;--muted:#6b6b66;--line:#e2e1dc;--card:#fff;--ok:#1f7a3a;--bad:#b3261e;--warn:#8a5a00;--native:#3b5bdb;--merged:#b8590f;--bar:#e9e8e3}}
@media (prefers-color-scheme: dark){{:root{{--bg:#151514;--fg:#ecebe6;--muted:#a3a29b;--line:#2e2e2b;--card:#1d1d1b;--ok:#5cc47a;--bad:#ff8a80;--warn:#e0b050;--native:#8ea6ff;--merged:#f0a060;--bar:#2e2e2b}}}}
body{{background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,Segoe UI,sans-serif;margin:0;padding:24px 16px}}
main{{max-width:1100px;margin:0 auto}} h1{{font-size:1.5rem;margin:0 0 4px}} h2{{font-size:1.1rem;margin:28px 0 8px}}
.muted{{color:var(--muted)}} .ok{{color:var(--ok)}} .bad{{color:var(--bad)}} .warn{{color:var(--warn);font-weight:600}}
.cards{{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:12px}}
.card{{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px 14px}}
.card b{{display:block;font-size:1.3rem;font-variant-numeric:tabular-nums}}
.bar{{background:var(--bar);border-radius:4px;height:8px;margin:6px 0}} .bar i{{display:block;height:8px;border-radius:4px;background:var(--fg)}}
.scroll{{overflow-x:auto}} table{{border-collapse:collapse;width:100%;font-size:.9rem}}
th,td{{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);white-space:nowrap}} td.num{{text-align:right;font-variant-numeric:tabular-nums}}
.kind{{font-weight:600}} .kind.native{{color:var(--native)}} .kind.merged{{color:var(--merged)}}
code{{font-size:.85rem}} a{{color:inherit}}
</style></head><body><main>
<h1>Sedra testnet status</h1>
<p class="muted">Network <code>sedra-testnet-10</code>. Test coins have no value. Nothing here concerns Sedra mainnet.
Updated {utc(s['generated'])}, monitor data from {utc(s['data_time']) if s['data_time'] else 'never'}{report}.</p>
{stale}
<div class="cards">
<div class="card">Phase<b>{e(s['phase'])}</b></div>
<div class="card">Virtual DAA score<b>{s['virtual_daa_score']:,}</b><span class="muted">{rate} per second, last hour</span></div>
<div class="card">Self-parent ban from DAA {s['ban_start']:,}<div class="bar"><i style="width:{pct(s['ban_start']):.1f}%"></i></div><span class="muted">{e(s['ban_start_eta'])}</span></div>
<div class="card">Merged mining from DAA {s['activation']:,}<div class="bar"><i style="width:{pct(s['activation']):.1f}%"></i></div><span class="muted">{e(s['activation_eta'])}</span></div>
</div>
<h2>Blocks</h2>
<div class="cards">
<div class="card">Native, all time<b>{s['blocks_all_time'].get('native', 0):,}</b><span class="muted">last 24 h: {s['blocks_last_24h'].get('native', 0):,}</span></div>
<div class="card">Merged-mined, all time<b>{s['blocks_all_time'].get('merged', 0):,}</b><span class="muted">last 24 h: {s['blocks_last_24h'].get('merged', 0):,}</span></div>
<div class="card">First merged-mined block<b style="font-size:1rem">{first_txt}</b></div>
<div class="card">SDM1 coinbases after the ban start<b class="{'bad' if late_sdm1 else 'ok'}">{late_sdm1}</b><span class="muted">must stay 0</span></div>
</div>
<h2>Nodes</h2>
<p class="muted">"Has the reference chain": the node has the newest reference chain block that is at least 100 DAA deep, on its
selected chain. An upgraded node should always say yes; the old node should say no from the first merged-mined block on.</p>
<div class="scroll"><table><tr><th>node</th><th>role</th><th>reachable</th><th>synced</th><th>DAA</th><th>peers</th><th>version</th><th>has the reference chain</th><th>first miss (DAA)</th></tr>{rows_nodes}</table></div>
<h2>Recent blocks</h2>
<p class="muted">native = its own proof of work meets the Sedra target; merged = accepted through a Kaspa testnet-10 parent (SIP-0001).</p>
<div class="scroll"><table><tr><th>recorded</th><th>DAA</th><th>hash</th><th>kind</th><th>coinbase tag</th><th></th></tr>{rows_blocks}</table></div>
<h2>Share per coinbase tag (all time)</h2>
<div class="scroll"><table><tr><th>tag</th><th>kind</th><th>blocks</th><th>share</th></tr>{rows_tags}</table></div>
<h2>Join</h2>
<p>Seed nodes (add with <code>--addpeer</code>):</p><ul>{seeds}</ul>
<p>Merged-mining pool: {pool}. Guide: <a href="https://github.com/necatgit/sedra-project/blob/main/testnet/GUIDE.md">testnet/GUIDE.md</a>.
Raw data: <a href="status.json">status.json</a>.</p>
</main></body></html>
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("monitor_dir")
    ap.add_argument("--out", required=True, help="directory the web server serves")
    ap.add_argument("--activation", type=int, default=345_600, help="merged-mining activation DAA (release 0.2.0-testnet.1: 345600)")
    ap.add_argument("--finality", type=int, default=86_400, help="finality depth (testnet: 86400)")
    ap.add_argument("--old", action="append", default=[], help="name of a node without merged mining (repeatable)")
    ap.add_argument("--seed", action="append", default=[], help="public seed address to list (repeatable)")
    ap.add_argument("--pool", default="", help="public stratum address of the testnet pool")
    ap.add_argument("--report-url", default="", help="link to the reports (for example issue #13)")
    build(ap.parse_args())


if __name__ == "__main__":
    main()
