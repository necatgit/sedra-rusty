//! SEDRA (M3): real Sedra mainnet headers, validated offline against the Rust implementation.
//!
//! `testdata/sedra_headers_2000.json.gz` was exported from a local sedrad v2.0.0 node (Go, synced from the public
//! Sedra network) with its `GetBlock` RPC: every block in a contiguous DAG section of 2000 DAA scores, including
//! non-chain blocks, with all header fields in their original order and the hash the Go node reports.
//!
//! Checked for every block:
//! 1. the header hash computed here equals the Go node's hash (identical serialization and hashing);
//! 2. the block passes the proof-of-work check against its own `bits` (identical kHeavyHash and target conversion);
//! 3. every parent at level `i` that is in the section has a computed block level of at least `i`
//!    (block levels computed from proof of work agree with the parent structure the Go node built).
//!
//! Not checked here: GHOSTDAG ordering, DAA scores and difficulty recalculation, which need the full chain context
//! (milestone M5, full sync against the Go node).

use crate::params::MAINNET_PARAMS;
use flate2::read::GzDecoder;
use kaspa_consensus_core::{header::Header, BlueWorkType};
use kaspa_hashes::Hash;
use std::{collections::HashMap, io::Read, str::FromStr};

fn hash(s: &str) -> Hash {
    Hash::from_str(s).unwrap()
}

fn num(v: &serde_json::Value) -> u64 {
    match v {
        serde_json::Value::String(s) => s.parse().unwrap(),
        other => other.as_u64().unwrap(),
    }
}

fn load() -> (serde_json::Value, Vec<(Hash, Header)>) {
    let mut json = String::new();
    GzDecoder::new(&include_bytes!("testdata/sedra_headers_2000.json.gz")[..]).read_to_string(&mut json).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
    let blocks = doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            let h = &b["header"];
            let parents: Vec<Vec<Hash>> = h["parents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|lvl| lvl["parentHashes"].as_array().unwrap().iter().map(|p| hash(p.as_str().unwrap())).collect())
                .collect();
            let header = Header::new_finalized(
                num(&h["version"]) as u16,
                parents,
                hash(h["hashMerkleRoot"].as_str().unwrap()),
                hash(h["acceptedIdMerkleRoot"].as_str().unwrap()),
                hash(h["utxoCommitment"].as_str().unwrap()),
                num(&h["timestamp"]),
                num(&h["bits"]) as u32,
                num(&h["nonce"]),
                num(&h["daaScore"]),
                BlueWorkType::from_hex(h["blueWork"].as_str().unwrap()).unwrap(),
                num(&h["blueScore"]),
                hash(h["pruningPoint"].as_str().unwrap()),
            );
            (hash(b["hash"].as_str().unwrap()), header)
        })
        .collect();
    (doc, blocks)
}

#[test]
fn sedra_real_headers_hash_pow_and_levels_test() {
    let (doc, blocks) = load();
    assert_eq!(blocks.len(), 2001, "export size");
    let max_level = MAINNET_PARAMS.max_block_level;

    // 1 + 2: hash parity and proof of work
    let mut levels: HashMap<Hash, u8> = HashMap::new();
    for (expected, header) in &blocks {
        assert_eq!(header.hash, *expected, "header hash differs from the Go node (DAA score {})", header.daa_score);
        let (level, passed) = kaspa_pow::calc_block_level_check_pow(header, max_level);
        assert!(passed, "proof of work fails for {} (DAA score {})", expected, header.daa_score);
        levels.insert(*expected, level);
    }

    // 3: parents at level i (within the section) have a block level of at least i
    let mut checked = 0usize;
    for (h, header) in &blocks {
        for (i, lvl) in header.parents_by_level.iter().enumerate() {
            for p in lvl {
                if let Some(&pl) = levels.get(p) {
                    assert!(pl as usize >= i, "block {h} lists {p} at level {i}, but its computed level is {pl}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 0);

    let lmin = levels.values().min().unwrap();
    let lmax = levels.values().max().unwrap();
    println!(
        "sedra real headers: {} blocks, tip DAA {}, {} parent-level references checked, block levels {}..{}",
        blocks.len(),
        num(&doc["tip_virtual_daa_score"]),
        checked,
        lmin,
        lmax
    );
}
