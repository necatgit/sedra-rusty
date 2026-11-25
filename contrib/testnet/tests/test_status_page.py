#!/usr/bin/env python3
"""SEDRA (#13): tests of contrib/testnet/status_page.py against synthetic monitor data (tests/fixtures.py).

Run: python3 -m unittest discover -s contrib/testnet/tests -v
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import fixtures  # noqa: E402

PAGE = os.path.join(HERE, "..", "status_page.py")


def run_page(mon, www, *extra):
    subprocess.run([sys.executable, PAGE, mon, "--out", www, "--old", "old-go", *extra], check=True)
    with open(os.path.join(www, "status.json")) as f:
        status = json.load(f)
    with open(os.path.join(www, "index.html"), encoding="utf-8") as f:
        page = f.read()
    return status, page


class StatusPageTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.mon = os.path.join(self.tmp.name, "mon")
        self.www = os.path.join(self.tmp.name, "www")

    def tearDown(self):
        self.tmp.cleanup()

    def crit(self, status):
        return {c["n"]: c["status"] for c in status["criteria"]}

    def test_native_phase(self):
        fixtures.make(self.mon, "native", step=50)
        s, page = run_page(self.mon, self.www)
        self.assertEqual(s["phase"], "Native phase")
        self.assertTrue(s["test_network"])
        self.assertEqual(s["blocks_all_time"].get("merged", 0), 0)
        self.assertFalse(s["ban_start_eta"]["reached"])
        self.assertIsNotNone(s["ban_start_eta"]["unix"])  # rate known: an ETA
        c = self.crit(s)
        self.assertEqual((c[1], c[2], c[3], c[8]), ("waiting", "waiting", "waiting", "waiting"))
        self.assertIn("TESTNET", page)
        self.assertIn("test coins, no value", page)
        self.assertIsNotNone(s["hashes_per_second"])
        for asset in ("status.css", "status.js", "logo.svg", "sedra.svg", "kaspa.svg"):
            self.assertTrue(os.path.exists(os.path.join(self.www, "status_assets", asset)), asset)
        self.assertFalse(os.path.exists(os.path.join(self.www, "status_assets", "criteria.example.json")))
        # without --zkas on the monitor: no ZKas section, criterion 9 open
        self.assertIsNone(s["zkas"])
        self.assertEqual(c[9], "open")
        self.assertNotIn("ZKas test chain", page)

    def test_ban_phase(self):
        fixtures.make(self.mon, "ban", step=50)
        s, _ = run_page(self.mon, self.www)
        self.assertIn("self-parent ban", s["phase"])
        self.assertTrue(s["ban_start_eta"]["reached"])
        self.assertEqual(self.crit(s)[1], "running")

    def test_merged_phase_old_node_stops(self):
        info = fixtures.make(self.mon, "merged", step=20)
        s, page = run_page(self.mon, self.www)
        self.assertEqual(s["phase"], "Merged mining active")
        self.assertEqual(s["first_merged_block"]["daa_score"], info["first_merged"])
        c = self.crit(s)
        self.assertEqual(c[1], "met")
        self.assertEqual(c[2], "met")
        self.assertEqual(c[3], "met")  # old-go missed from the first merged-mined block on: as predicted
        self.assertEqual(c[8], "running")
        old = [n for n in s["nodes"] if n["node"] == "old-go"][0]
        self.assertGreaterEqual(old["first_miss_daa"], info["first_merged"])
        self.assertIn("merged-mined", page)

    def test_zkas_shared_parents_after_activation(self):
        fixtures.make(self.mon, "merged", step=40, zkas=True)
        with open(os.path.join(self.mon, "shared_parents.csv")) as f:
            pairs = len(f.readlines()) - 1
        s, page = run_page(self.mon, self.www)
        self.assertGreater(pairs, 0)
        self.assertEqual(s["shared_parents"]["count"], pairs)
        self.assertEqual(len(s["shared_parents"]["recent"]), 8)
        self.assertEqual(self.crit(s)[9], "met")
        self.assertGreater(s["zkas"]["blocks_all_time"]["merged"], 0)
        self.assertTrue(s["zkas"]["reachable"])
        self.assertIn("ZKas test chain", page)
        self.assertIn("zkas.svg", page)
        # every pair shown is a Sedra block whose nonce is the parent prefix
        with open(os.path.join(self.mon, "blocks.csv")) as f:
            nonce = {r.split(",")[1]: r.strip().split(",")[-1] for r in f.readlines()[1:]}
        for p in s["shared_parents"]["recent"]:
            self.assertEqual(nonce[p["sedra_hash"]], p["parent_prefix"])

    def test_zkas_before_activation_waits(self):
        fixtures.make(self.mon, "ban", step=100, zkas=True)
        s, page = run_page(self.mon, self.www)
        self.assertEqual(s["shared_parents"]["count"], 0)
        self.assertEqual(self.crit(s)[9], "waiting")
        self.assertIn("None yet", page)

    def test_zkas_operator_can_fail_criterion_9(self):
        fixtures.make(self.mon, "merged", step=200, zkas=True)
        crit = os.path.join(self.tmp.name, "criteria.json")
        with open(crit, "w") as f:
            json.dump({"9": {"status": "failed", "note": "ZKas rejected a block because of the SDM1 tag", "link": ""}}, f)
        s, _ = run_page(self.mon, self.www, "--criteria", crit)
        self.assertEqual(self.crit(s)[9], "failed")

    def test_wide_tables_do_not_widen_the_page(self):
        # regression 2026-11-25: the shared-parents table in a chart-grid cell made the phone page 620 px wide;
        # grid cells must be allowed to shrink so the table scrolls inside its .scroll box
        with open(os.path.join(HERE, "..", "status_assets", "status.css"), encoding="utf-8") as f:
            css = f.read()
        self.assertIn(".chart-grid > div { min-width: 0; }", css)

    def test_upgraded_node_miss_fails_criteria(self):
        fixtures.make(self.mon, "merged", step=40)
        with open(os.path.join(self.mon, "agreement.csv"), "a") as f:
            f.write(f"{info_time()},{'ab' * 32},{fixtures.ACTIVATION + 1000},seed2,0,0\n")
        s, _ = run_page(self.mon, self.www)
        c = self.crit(s)
        self.assertEqual((c[1], c[2], c[8]), ("failed", "failed", "failed"))

    def test_sdm1_after_ban_start_fails_criterion_1(self):
        fixtures.make(self.mon, "ban", step=100)
        with open(os.path.join(self.mon, "blocks.csv"), "a") as f:
            f.write(f"{info_time()},{'cd' * 32},{fixtures.BAN + 10},1,1,native,1,0,{'ef' * 32},00,0.2.0-testnet.1/SDM1 test\n")
        s, _ = run_page(self.mon, self.www)
        self.assertEqual(self.crit(s)[1], "failed")

    def test_incremental_equals_full_recount(self):
        fixtures.make(self.mon, "merged", step=40, zkas=True)
        saved = {}
        for name in ("blocks.csv", "zkas_blocks.csv", "shared_parents.csv"):
            path = os.path.join(self.mon, name)
            with open(path) as f:
                saved[path] = f.readlines()
            with open(path, "w") as f:
                f.writelines(saved[path][:len(saved[path]) // 2])
        run_page(self.mon, self.www)
        for path, lines in saved.items():
            with open(path, "a") as f:
                f.writelines(lines[len(lines) // 2:])
        s_inc, _ = run_page(self.mon, self.www)
        os.remove(os.path.join(self.www, ".status-state.json"))
        s_full, _ = run_page(self.mon, self.www)
        for k in ("blocks_all_time", "share_per_tag", "first_merged_block", "blocks_since_activation", "shared_parents"):
            self.assertEqual(s_inc[k], s_full[k], k)
        self.assertEqual(s_inc["zkas"]["blocks_all_time"], s_full["zkas"]["blocks_all_time"])

    def test_operator_criteria_and_escaping(self):
        fixtures.make(self.mon, "native", step=100)
        crit = os.path.join(self.tmp.name, "criteria.json")
        with open(crit, "w") as f:
            json.dump({"6": {"status": "met", "note": "<script>alert(1)</script>", "link": "https://example.org/x"},
                       "7": {"status": "met", "link": "javascript:alert(1)"},
                       "5": {"status": "bogus"}}, f)
        s, page = run_page(self.mon, self.www, "--criteria", crit, "--pool", "stratum+tcp://<b>x</b>:1")
        c = self.crit(s)
        self.assertEqual((c[5], c[6], c[7]), ("open", "met", "met"))
        self.assertNotIn("<script>alert", page)
        self.assertNotIn("<b>x</b>", page)
        self.assertNotIn("javascript:", page)
        self.assertIn('href="https://example.org/x"', page)

    def test_caddy_csp_allows_the_inline_script(self):
        import base64
        import hashlib
        import re
        fixtures.make(self.mon, "native", step=200)
        _, page = run_page(self.mon, self.www)
        inline = re.findall(r"<script>(.*?)</script>", page, re.S)
        self.assertEqual(len(inline), 1)
        digest = base64.b64encode(hashlib.sha256(inline[0].encode()).digest()).decode()
        with open(os.path.join(HERE, "..", "web", "Caddyfile")) as f:
            self.assertIn(f"'sha256-{digest}'", f.read())

    def test_publish_skipped_without_ssh_master(self):
        fixtures.make(self.mon, "native", step=200)
        env = dict(os.environ, STATUS_PUSH_SOCKET=os.path.join(self.tmp.name, "no-such-socket"))
        r = subprocess.run([sys.executable, PAGE, self.mon, "--out", self.www, "--publish", "nobody@invalid.invalid:/"],
                           env=env, capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("not published", r.stdout)
        self.assertTrue(os.path.exists(os.path.join(self.www, "index.html")))

    def test_no_data_yet(self):
        os.makedirs(self.mon)
        s, page = run_page(self.mon, self.www)
        self.assertEqual(s["virtual_daa_score"], 0)
        self.assertTrue(s["stale"])
        self.assertIn("No blocks yet", page)


def info_time():
    import time
    return int(time.time()) - 30


if __name__ == "__main__":
    unittest.main()
