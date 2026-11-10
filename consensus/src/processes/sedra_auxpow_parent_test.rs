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

// ---- M10 step 2: the whole proof on a real Kaspa block ----

use kaspa_consensus_core::auxpow::{AuxPow, AuxPowError, MAX_COINBASE_MERKLE_BRANCH};

/// A Sedra header and a proof whose parent is a real Kaspa block (one with a Merkle branch) whose coinbase payload
/// additionally carries the Sedra commitment. The parent's root changes with the payload, so its proof of work no
/// longer holds; that is checked in the work step, not here.
fn bound_example() -> (Hash, Header, AuxPow) {
    example(0x1d00ffff, None)
}

/// A Sedra header with `bits`, bound to a real Kaspa parent that commits to it. With `parent_meets_target` set, the
/// parent's nonce is searched so that its kHeavyHash meets (true) or misses (false) the Sedra target.
fn example(bits: u32, parent_meets_target: Option<bool>) -> (Hash, Header, AuxPow) {
    let genesis = MAINNET_PARAMS.genesis.hash;
    let v = coinbases().into_iter().find(|v| v.tx_hashes.len() >= 3).unwrap();
    let mut parent = headers().into_iter().find(|(h, _)| *h == v.block_hash).unwrap().1;
    let mut sedra = Header::new_finalized(1, vec![vec![genesis]], ZERO_HASH, ZERO_HASH, ZERO_HASH, 1_794_000_000_000, bits, 0, 61_500_000, 0.into(), 61_000_000, genesis);

    let mut coinbase = v.coinbase.clone();
    coinbase.payload.extend_from_slice(&AuxPow::tagged_commitment(AuxPow::commitment(genesis, AuxPow::nonce_free_header_hash(&sedra))));
    let branch = leaf0_branch(&v.tx_hashes);
    parent.hash_merkle_root = merkle_root_from_coinbase(coinbase.hash().unwrap(), &branch);
    if let Some(want) = parent_meets_target {
        let target = Uint256::from_compact_target_bits(bits);
        let state = kaspa_pow::State::new(&parent);
        parent.nonce = (0u64..).find(|&n| (state.calculate_pow(n) <= target) == want).unwrap();
    }
    parent.finalize();

    sedra.nonce = AuxPow::nonce_binding(&parent);
    sedra.finalize();
    (genesis, sedra, AuxPow { parent_header: parent, parent_coinbase: coinbase, coinbase_merkle_branch: branch })
}

#[test]
fn sedra_auxpow_binding_on_real_kaspa_block_test() {
    let (genesis, sedra, aux) = bound_example();
    assert_eq!(aux.verify_binding(genesis, &sedra), Ok(()));
    assert!(!aux.coinbase_merkle_branch.is_empty());

    // the nonce is outside the commitment, everything else is inside
    let mut s = sedra.clone();
    s.nonce += 1;
    assert!(matches!(aux.verify_binding(genesis, &s), Err(AuxPowError::NonceNotBound { .. })), "nonce");
    let mut s = sedra.clone();
    s.timestamp += 1;
    assert_eq!(aux.verify_binding(genesis, &s), Err(AuxPowError::WrongCommitment), "timestamp is bound");
    let mut s = sedra.clone();
    s.bits += 1;
    assert_eq!(aux.verify_binding(genesis, &s), Err(AuxPowError::WrongCommitment), "bits are bound");
    assert_eq!(aux.verify_binding(ZERO_HASH, &sedra), Err(AuxPowError::WrongCommitment), "another network's genesis");

    // the nonce is bound to this parent: another parent (here: another header field) changes it
    let mut a = aux.clone();
    a.parent_header.timestamp += 1;
    a.parent_header.finalize();
    assert!(matches!(a.verify_binding(genesis, &sedra), Err(AuxPowError::NonceNotBound { .. })), "nonce bound to the parent");

    // coinbase and branch
    let mut a = aux.clone();
    a.coinbase_merkle_branch[0] = ZERO_HASH;
    assert_eq!(a.verify_binding(genesis, &sedra), Err(AuxPowError::NotInParentMerkleRoot), "branch");
    let mut a = aux.clone();
    a.parent_coinbase.outputs[0].value += 1;
    assert_eq!(a.verify_binding(genesis, &sedra), Err(AuxPowError::NotInParentMerkleRoot), "coinbase changed");
    let mut a = aux.clone();
    let tag = AuxPow::tagged_commitment(AuxPow::commitment(genesis, AuxPow::nonce_free_header_hash(&sedra)));
    a.parent_coinbase.payload.extend_from_slice(&tag);
    assert_eq!(a.verify_binding(genesis, &sedra), Err(AuxPowError::NoUniqueCommitment), "two commitments");
    let mut a = aux.clone();
    a.parent_coinbase.version = 2;
    assert_eq!(a.verify_binding(genesis, &sedra), Err(AuxPowError::UnhashableCoinbase(2)), "unknown version");

    // limits
    let mut a = aux.clone();
    a.coinbase_merkle_branch = vec![ZERO_HASH; MAX_COINBASE_MERKLE_BRANCH + 1];
    assert_eq!(a.verify_binding(genesis, &sedra), Err(AuxPowError::BranchTooLong(MAX_COINBASE_MERKLE_BRANCH + 1)), "branch limit");
    let mut a = aux.clone();
    a.parent_coinbase.payload.extend(std::iter::repeat(0u8).take(300 * 1024));
    assert!(matches!(a.verify_binding(genesis, &sedra), Err(AuxPowError::TooLarge(_))), "size limit");

    // self-parent ban input: the parent's coinbase carries a commitment, a plain Kaspa coinbase does not
    assert!(AuxPow::payload_carries_commitment(&aux.parent_coinbase.payload));
    assert!(coinbases().iter().all(|v| !AuxPow::payload_carries_commitment(&v.coinbase.payload)));
}

#[test]
fn sedra_auxpow_borsh_roundtrip_test() {
    let (genesis, sedra, aux) = bound_example();
    let bytes = borsh::to_vec(&aux).unwrap();
    let decoded: AuxPow = borsh::from_slice(&bytes).unwrap();
    assert_eq!(decoded.verify_binding(genesis, &sedra), Ok(()));
    assert_eq!(borsh::to_vec(&decoded).unwrap(), bytes);
    assert!(bytes.len() < 16 * 1024, "a real proof is a few kilobytes: {}", bytes.len());
}

// ---- M10 step 3: the parent's work against the Sedra target, dual acceptance, block level (§4.4, §4.7) ----

use kaspa_math::Uint256;
use kaspa_pow::auxpow::{calc_block_level_check_pow_gated, check_pow_gated, parent_pow, verify_aux_pow, AuxPowRejection, AuxRule, PowCheck};

/// Sedra target 2^244: about one kHeavyHash in 4096 meets it, so the searches below stay fast.
const AUX_BITS: u32 = 0x1f0fffff;

#[test]
fn sedra_auxpow_work_test() {
    let (genesis, sedra, aux) = example(AUX_BITS, Some(true));
    let target = Uint256::from_compact_target_bits(AUX_BITS);
    let pow = parent_pow(&aux);
    assert!(pow <= target);
    assert!(!kaspa_pow::State::new(&sedra).check_pow(sedra.nonce).0, "the aux path must be what decides");
    // the parent misses Kaspa's own (much harder) target: only the Sedra target counts (§4.4)
    assert!(!kaspa_pow::State::new(&aux.parent_header).check_pow(aux.parent_header.nonce).0);

    assert_eq!(verify_aux_pow(&aux, genesis, &sedra), Ok(pow));
    assert_eq!(check_pow_gated(&sedra, Some(&aux), AuxRule::new(true, genesis)), PowCheck::Aux(pow));

    // before activation, or without a proof, the block is a plain native failure
    let off = check_pow_gated(&sedra, Some(&aux), AuxRule::new(false, genesis));
    assert!(matches!(off, PowCheck::Failed { aux: None, .. }), "inactive gate ignores the proof: {off:?}");
    assert!(matches!(check_pow_gated(&sedra, None, AuxRule::new(true, genesis)), PowCheck::Failed { aux: None, .. }));

    // a proof for another network
    let other = check_pow_gated(&sedra, Some(&aux), AuxRule::new(true, ZERO_HASH));
    assert!(matches!(other, PowCheck::Failed { aux: Some(AuxPowRejection::Binding(AuxPowError::WrongCommitment)), .. }));

    // a correctly bound parent whose work misses the Sedra target
    let (genesis, weak_sedra, weak) = example(AUX_BITS, Some(false));
    assert!(parent_pow(&weak) > target);
    assert_eq!(weak.verify_binding(genesis, &weak_sedra), Ok(()));
    assert_eq!(verify_aux_pow(&weak, genesis, &weak_sedra), Err(AuxPowRejection::InsufficientWork));
    let check = check_pow_gated(&weak_sedra, Some(&weak), AuxRule::new(true, genesis));
    assert!(matches!(check, PowCheck::Failed { aux: Some(AuxPowRejection::InsufficientWork), .. }));
    assert!(!check.passed());
}

#[test]
fn sedra_auxpow_native_first_test() {
    let (genesis, mut sedra, aux) = example(AUX_BITS, Some(true));
    // mine the Sedra header natively: its nonce no longer matches the parent, so the attached proof is invalid
    let state = kaspa_pow::State::new(&sedra);
    sedra.nonce = (0u64..).find(|&n| state.check_pow(n).0).unwrap();
    sedra.finalize();
    assert!(matches!(aux.verify_binding(genesis, &sedra), Err(AuxPowError::NonceNotBound { .. })));
    let (_, native_pow) = state.check_pow(sedra.nonce);

    // a bad or missing proof never invalidates a natively valid block, before or after activation
    for rule in [AuxRule::new(true, genesis), AuxRule::new(false, genesis), AuxRule::new(true, ZERO_HASH)] {
        assert_eq!(check_pow_gated(&sedra, Some(&aux), rule), PowCheck::Native(native_pow));
        assert_eq!(check_pow_gated(&sedra, None, rule), PowCheck::Native(native_pow));
    }
}

#[test]
fn sedra_auxpow_block_level_test() {
    // level = max - bits(pow). Sedra mainnet uses 225 against targets near 2^200; the test target is 2^244, so a
    // higher max keeps both levels above 0 (the difference between them is the same)
    let max = 255;
    let (genesis, sedra, aux) = example(AUX_BITS, Some(true));
    let active = AuxRule::new(true, genesis);

    // an aux-accepted block's level comes from the parent's work (§4.7)
    let (level, ok) = calc_block_level_check_pow_gated(&sedra, max, Some(&aux), active);
    assert!(ok);
    assert_eq!(level, kaspa_pow::calc_level_from_pow(parent_pow(&aux), max));

    // the native-only calculation rejects the block and computes a lower level: proof validation must use the gate
    let (native_level, native_ok) = kaspa_pow::calc_block_level_check_pow(&sedra, max);
    assert!(!native_ok);
    assert!(native_level < level, "native {native_level} vs aux {level}");

    // before activation the gate gives exactly the native result
    assert_eq!(calc_block_level_check_pow_gated(&sedra, max, Some(&aux), AuxRule::new(false, genesis)), (native_level, false));

    // genesis keeps the max level
    let g = Header::new_finalized(1, vec![], ZERO_HASH, ZERO_HASH, ZERO_HASH, 0, AUX_BITS, 0, 0, 0.into(), 0, ZERO_HASH);
    assert_eq!(calc_block_level_check_pow_gated(&g, max, None, active), (max, true));
}
