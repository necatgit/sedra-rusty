//! SEDRA (M10, SIP-0001 §4.6, Q19): Kaspa parent blocks are hashed exactly as Kaspa hashes them.
//!
//! Real Kaspa mainnet data, 20 consecutive selected-chain blocks:
//! - `testing/integration/testdata/kaspa_mainnet_headers_v2.json.gz`: headers (block version 2) with full parents by
//!   level and the hash a Kaspa node (kaspad 2.1.0) reports, from its `GetBlock` RPC;
//! - `testing/integration/testdata/kaspa_mainnet_coinbases_v1.json.gz`: for the same blocks, the coinbase (transaction
//!   version 1 since Toccata), the hash Kaspa reports for it and for every transaction, and `hashMerkleRoot`, from the
//!   public REST API api.kaspa.org.
//!
//! Checked for every block: the header hash, the header's kHeavyHash against its own `bits`, the coinbase hash, and
//! that the coinbase reaches `hashMerkleRoot` through its Merkle branch. Expected values all come from Kaspa, never
//! from this code. Each negative test breaks one input and must fail.

use crate::params::MAINNET_PARAMS;
use flate2::read::GzDecoder;
use kaspa_consensus_core::{
    header::Header,
    kaspa_parent::{merkle_root_from_coinbase, KaspaCovenant, KaspaParentCoinbase, KaspaParentOutput},
    subnets::SubnetworkId,
    tx::{ScriptPublicKey, ScriptVec},
    BlueWorkType,
};
use kaspa_hashes::{Hash, ZERO_HASH};
use std::{io::Read, str::FromStr};

const HEADERS: &[u8] = include_bytes!("../../../testing/integration/testdata/kaspa_mainnet_headers_v2.json.gz");
const COINBASES: &[u8] = include_bytes!("../../../testing/integration/testdata/kaspa_mainnet_coinbases_v1.json.gz");

fn json(gz: &[u8]) -> serde_json::Value {
    let mut s = String::new();
    GzDecoder::new(gz).read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn hash(v: &serde_json::Value) -> Hash {
    Hash::from_str(v.as_str().unwrap()).unwrap()
}

fn num(v: &serde_json::Value) -> u64 {
    match v {
        serde_json::Value::String(s) => s.parse().unwrap(),
        other => other.as_u64().unwrap(),
    }
}

fn unhex(v: &serde_json::Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn headers() -> Vec<(Hash, Header)> {
    json(HEADERS)["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            let h = &b["header"];
            let parents: Vec<Vec<Hash>> = h["parents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|lvl| lvl["parentHashes"].as_array().unwrap().iter().map(hash).collect())
                .collect();
            let header = Header::new_finalized(
                num(&h["version"]) as u16,
                parents,
                hash(&h["hashMerkleRoot"]),
                hash(&h["acceptedIdMerkleRoot"]),
                hash(&h["utxoCommitment"]),
                num(&h["timestamp"]),
                num(&h["bits"]) as u32,
                num(&h["nonce"]),
                num(&h["daaScore"]),
                BlueWorkType::from_hex(h["blueWork"].as_str().unwrap()).unwrap(),
                num(&h["blueScore"]),
                hash(&h["pruningPoint"]),
            );
            (hash(&b["hash"]), header)
        })
        .collect()
}

struct CoinbaseVector {
    block_hash: Hash,
    hash_merkle_root: Hash,
    coinbase: KaspaParentCoinbase,
    coinbase_hash: Hash,
    tx_hashes: Vec<Hash>,
}

fn coinbases() -> Vec<CoinbaseVector> {
    json(COINBASES)["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            let c = &b["coinbase"];
            assert_eq!(num(&c["mass"]), 0, "a Kaspa coinbase has mass 0");
            let outputs = c["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| KaspaParentOutput {
                    value: num(&o["value"]),
                    script_public_key: ScriptPublicKey::new(num(&o["script_version"]) as u16, ScriptVec::from_vec(unhex(&o["script"]))),
                    covenant: if o["covenant"].is_null() {
                        None
                    } else {
                        Some(KaspaCovenant {
                            authorizing_input: num(&o["covenant"]["authorizingInput"]) as u16,
                            covenant_id: hash(&o["covenant"]["covenantId"]),
                        })
                    },
                })
                .collect();
            CoinbaseVector {
                block_hash: hash(&b["block_hash"]),
                hash_merkle_root: hash(&b["hash_merkle_root"]),
                coinbase: KaspaParentCoinbase {
                    version: num(&c["version"]) as u16,
                    outputs,
                    lock_time: num(&c["lock_time"]),
                    subnetwork_id: SubnetworkId::from_str(c["subnetwork_id"].as_str().unwrap()).unwrap(),
                    gas: num(&c["gas"]),
                    payload: unhex(&c["payload"]),
                },
                coinbase_hash: hash(&b["coinbase_hash"]),
                tx_hashes: b["tx_hashes"].as_array().unwrap().iter().map(hash).collect(),
            }
        })
        .collect()
}

/// The right siblings from leaf 0 to the root, built like Kaspa's `calc_merkle_root` (a missing right child is the
/// zero hash).
fn leaf0_branch(leaves: &[Hash]) -> Vec<Hash> {
    let mut level: Vec<Option<Hash>> = leaves.iter().copied().map(Some).collect();
    level.resize(leaves.len().next_power_of_two(), None);
    let mut branch = Vec::new();
    while level.len() > 1 {
        branch.push(level[1].unwrap_or(ZERO_HASH));
        level = level.chunks(2).map(|p| p[0].map(|l| kaspa_merkle::merkle_hash(l, p[1].unwrap_or(ZERO_HASH)))).collect();
    }
    branch
}

#[test]
fn sedra_kaspa_parent_headers_hash_and_pow_test() {
    let blocks = headers();
    assert_eq!(blocks.len(), 20);
    for (expected, header) in &blocks {
        assert_eq!(header.version, 2, "Kaspa headers are block version 2");
        assert_eq!(header.hash, *expected, "Kaspa header hash differs (DAA score {})", header.daa_score);
        let (_, passed) = kaspa_pow::calc_block_level_check_pow(header, MAINNET_PARAMS.max_block_level);
        assert!(passed, "kHeavyHash fails for Kaspa block {expected}");
    }
}

#[test]
fn sedra_kaspa_parent_coinbase_hash_test() {
    let vectors = coinbases();
    assert_eq!(vectors.len(), 20);
    for v in &vectors {
        assert_eq!(v.coinbase.version, 1, "Kaspa coinbases are transaction version 1 since Toccata");
        assert_eq!(v.coinbase.hash(), Some(v.coinbase_hash), "coinbase hash differs from Kaspa's (block {})", v.block_hash);
        assert_eq!(v.tx_hashes[0], v.coinbase_hash, "the coinbase is leaf 0");
    }
}

#[test]
fn sedra_kaspa_parent_coinbase_reaches_merkle_root_test() {
    let headers = headers();
    let mut depths = Vec::new();
    for v in coinbases() {
        let header = &headers.iter().find(|(h, _)| *h == v.block_hash).expect("same blocks in both files").1;
        assert_eq!(header.hash_merkle_root, v.hash_merkle_root);
        assert_eq!(kaspa_merkle::calc_merkle_root(v.tx_hashes.iter().copied()), v.hash_merkle_root, "Kaspa's tree from Kaspa's hashes");
        let branch = leaf0_branch(&v.tx_hashes);
        depths.push(branch.len());
        assert_eq!(merkle_root_from_coinbase(v.coinbase.hash().unwrap(), &branch), header.hash_merkle_root, "block {}", v.block_hash);
    }
    assert!(depths.contains(&0) && depths.iter().any(|&d| d >= 2), "vectors cover an empty and a deeper branch: {depths:?}");
}

#[test]
fn sedra_kaspa_parent_negative_test() {
    let v = &coinbases().into_iter().find(|v| v.tx_hashes.len() >= 3).expect("a block with a real branch");
    let good = v.coinbase.hash().unwrap();
    let branch = leaf0_branch(&v.tx_hashes);

    // every field is committed
    let mut c = v.coinbase.clone();
    c.payload[0] ^= 1;
    assert_ne!(c.hash(), Some(good), "payload");
    let mut c = v.coinbase.clone();
    c.outputs[0].value += 1;
    assert_ne!(c.hash(), Some(good), "output value");
    let mut c = v.coinbase.clone();
    c.outputs[0].covenant = Some(KaspaCovenant { authorizing_input: 0, covenant_id: ZERO_HASH });
    assert_ne!(c.hash(), Some(good), "covenant flag");
    let mut c = v.coinbase.clone();
    c.version = 0;
    assert_ne!(c.hash(), Some(good), "version 0 encodes differently");

    // formats this node cannot hash
    let mut c = v.coinbase.clone();
    c.version = 2;
    assert_eq!(c.hash(), None, "unknown future transaction version");
    let mut c = v.coinbase.clone();
    c.version = 0;
    c.outputs[0].covenant = Some(KaspaCovenant { authorizing_input: 0, covenant_id: ZERO_HASH });
    assert_eq!(c.hash(), None, "version 0 output with a covenant");

    // the branch must be exact
    assert_eq!(merkle_root_from_coinbase(good, &branch), v.hash_merkle_root);
    let mut b = branch.clone();
    b.reverse();
    assert_ne!(merkle_root_from_coinbase(good, &b), v.hash_merkle_root, "reordered branch");
    assert_ne!(merkle_root_from_coinbase(good, &branch[..branch.len() - 1]), v.hash_merkle_root, "shortened branch");
    assert_ne!(merkle_root_from_coinbase(v.tx_hashes[1], &branch), v.hash_merkle_root, "another transaction as leaf 0");
}
