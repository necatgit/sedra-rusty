#!/usr/bin/env python3
"""SEDRA (#13): synthetic sedra-testnet-monitor CSV files for the status page tests and screenshots.

Writes nodes.csv, blocks.csv, agreement.csv and network.csv as the monitor does, for a chain that ends "now" in one of
three phases: native (before the ban start), ban (between ban start and activation) or merged (activation passed,
merged-mined blocks next to native ones, the old Go node stopped at the first merged-mined block). One block row per
`step` DAA keeps the files small; the counts are therefore smaller than on the real network.

With zkas=True also zkas.csv, zkas_blocks.csv and shared_parents.csv as the monitor writes them with --zkas: a private
ZKas chain merged-mined from the start; after the activation about half of the merged Sedra blocks share their Kaspa
parent (and so their nonce) with a ZKas block.

Usage: python3 fixtures.py OUT_DIR native|ban|merged [--step 10] [--zkas]
"""
import argparse
import os
import random
import time

ACTIVATION = 345_600
BAN = ACTIVATION - 2 * 86_400
HDR = {
    "nodes.csv": "unix_time,node,status,network,virtual_daa_score,sink,pruning_point,block_count,is_synced,peers,server_version,error",
    "blocks.csv": "unix_time,hash,daa_score,blue_score,timestamp_ms,kind,is_chain_block,red,accepted_by,payout_script,extra_data,nonce",
    "agreement.csv": "unix_time,hash,daa_score,node,present,is_chain_block",
    "network.csv": "unix_time,virtual_daa_score,difficulty,hashes_per_second",
}
ZKAS_HDR = {
    "zkas.csv": "unix_time,node,status,network,virtual_daa_score,sink,block_count,is_synced,server_version,error",
    "zkas_blocks.csv": HDR["blocks.csv"],
    "shared_parents.csv": "unix_time,parent_prefix,sedra_hash,zkas_hash",
}
TIP = {"native": 120_000, "ban": 260_000, "merged": ACTIVATION + 150_000}


def h(rnd):
    return "%064x" % rnd.getrandbits(256)


def make(out, phase, step=10, seed=13, zkas=False):
    rnd = random.Random(seed)
    os.makedirs(out, exist_ok=True)
    hdr = HDR | (ZKAS_HDR if zkas else {})
    files = {k: open(os.path.join(out, k), "w") for k in hdr}
    for k, f in files.items():
        f.write(hdr[k] + "\n")
    now = int(time.time()) - 20
    tip = TIP[phase]
    t0 = now - tip  # about one DAA per second
    first_merged = None
    for daa in range(1, tip + 1, step):
        t = t0 + daa
        merged = daa >= ACTIVATION and rnd.random() < 0.8
        if merged and first_merged is None:
            first_merged = daa
        tag = rnd.choice(["puddlepool-testnet", "puddlepool-testnet", "tn10-friends"]) if merged else \
            rnd.choice(["sedra-testnet-miner", "sedra-testnet-miner", "cpu-alice", "sedraminer"])
        payout = "20" + "%064x" % rnd.randrange(6) + "ac"
        red = "1" if rnd.random() < 0.004 else "0"
        sedra_hash, nonce = h(rnd), "%016x" % rnd.getrandbits(64)
        files["blocks.csv"].write(f"{t},{sedra_hash},{daa},{daa - 3},{t * 1000},{'merged' if merged else 'native'},1,{red},"
                                  f"{h(rnd)},{payout},0.2.0-testnet.1/{tag},{nonce}\n")
        if zkas:
            # one ZKas block per row, always merged-mined; after the activation half share the Sedra block's parent
            shared = merged and rnd.random() < 0.5
            zhash, znonce = h(rnd), nonce if shared else "%016x" % rnd.getrandbits(64)
            files["zkas_blocks.csv"].write(f"{t},{zhash},{daa // 2},{daa // 2 - 3},{t * 1000},merged,1,0,{h(rnd)},"
                                           f"{payout},zkas-v1.0.14/puddlepool-testnet,{znonce}\n")
            if shared:
                files["shared_parents.csv"].write(f"{t},{nonce},{sedra_hash},{zhash}\n")
    # agreement: every 10 min over the whole run; the old node misses from the first merged-mined block on
    for t in range(t0 + 600, now, 600):
        daa = t - t0 - 100
        for node in ("seed2", "old-go"):
            ok = not (node == "old-go" and first_merged is not None and daa >= first_merged)
            files["agreement.csv"].write(f"{t},{h(rnd)},{daa},{node},{int(ok)},{int(ok)}\n")
    # nodes: every minute over the last two hours
    for t in range(now - 7200, now + 1, 60):
        daa = t - t0
        for node, ver, peers in (("seed1", "0.2.0-testnet.1", 7), ("seed2", "0.2.0-testnet.1", 9), ("old-go", "2.0.0", 3)):
            nd = min(daa, first_merged - 1) if node == "old-go" and first_merged else daa
            files["nodes.csv"].write(f"{t},{node},ok,sedra-testnet-10,{nd},{h(rnd)},{h(rnd)},{nd * 1.02:.0f},1,{peers},{ver},\n")
        if zkas:
            files["zkas.csv"].write(f"{t},zkas,ok,zkas,{daa // 2},{h(rnd)},{daa // 2},1,zkas-v1.0.14-1-g312c6fb7,\n")
    # network: every minute over the last 24 h; merged mining brings much more hashrate
    drift = 1.0
    for t in range(now - 86400, now + 1, 60):
        daa = t - t0
        base = 4.0e7 if daa >= ACTIVATION else 2.3e6
        drift = min(1.3, max(0.7, drift + rnd.uniform(-0.01, 0.01)))  # the node's estimate moves slowly
        hps = base * drift
        files["network.csv"].write(f"{t},{daa},{hps / 2.0:.3f},{hps:.0f}\n")
    for f in files.values():
        f.close()
    return {"tip": tip, "first_merged": first_merged, "now": now}


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("phase", choices=sorted(TIP))
    ap.add_argument("--step", type=int, default=10)
    ap.add_argument("--zkas", action="store_true")
    a = ap.parse_args()
    print(make(a.out, a.phase, a.step, zkas=a.zkas))
