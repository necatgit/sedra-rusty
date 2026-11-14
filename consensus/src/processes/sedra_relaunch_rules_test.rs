//! SEDRA (M6): the two sedrad v2.0.0 relaunch rules, checked against sedrad's own code.
//!
//! `testdata/sedrad_relaunch_vectors.json` was produced by calling sedrad v2.0.0's `difficultyManager.applyDifficultyReset`
//! and `coinbaseManager.treasuryAllocationOutput` (mainnet params, only the DAA score lookup stubbed) for DAA scores one
//! block before, at and after every window boundary, plus points inside, between and far outside the windows.

use crate::{
    model::stores::ghostdag::GhostdagData,
    params::MAINNET_PARAMS,
    processes::{coinbase::CoinbaseManager, window::apply_difficulty_reset},
};
use kaspa_consensus_core::{
    coinbase::{BlockRewardData, MinerData},
    tx::{ScriptPublicKey, ScriptVec},
    BlockHashMap, BlockHashSet,
};
use kaspa_hashes::Hash;

fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!("testdata/sedrad_relaunch_vectors.json")).unwrap()
}

fn mainnet_coinbase_manager() -> CoinbaseManager {
    let p = MAINNET_PARAMS;
    CoinbaseManager::new(
        p.coinbase_payload_script_public_key_max_len,
        p.max_coinbase_payload_len,
        p.deflationary_phase_daa_score,
        p.pre_deflationary_phase_base_subsidy,
        p.bps(),
    )
    .with_treasury_allocation(p.treasury_allocation_windows, p.treasury_script_public_key)
}

#[test]
fn sedra_difficulty_reset_parity_with_go_test() {
    let doc = vectors();
    let p = MAINNET_PARAMS;
    let computed = doc["computed_bits"].as_u64().unwrap() as u32;
    assert_eq!(doc["genesis_bits"].as_u64().unwrap() as u32, p.genesis.bits);
    let vecs = doc["vectors"].as_array().unwrap();
    assert_eq!(vecs.len(), 25);
    for v in vecs {
        let daa = v["daa_score"].as_u64().unwrap();
        let expected = v["bits_after_reset"].as_u64().unwrap() as u32;
        let got = apply_difficulty_reset(
            computed,
            daa,
            p.difficulty_reset_activation_daa_score,
            p.difficulty_reset_window_blocks,
            p.genesis.bits,
        );
        assert_eq!(got, expected, "difficulty reset at DAA score {daa}");
    }
}

#[test]
fn sedra_treasury_allocation_parity_with_go_test() {
    let doc = vectors();
    let cbm = mainnet_coinbase_manager();
    let mut paying = 0;
    for v in doc["vectors"].as_array().unwrap() {
        let daa = v["daa_score"].as_u64().unwrap();
        let got = cbm.treasury_allocation_output(daa);
        match v["treasury_value"].as_u64() {
            None => assert!(got.is_none(), "unexpected treasury output at DAA score {daa}"),
            Some(value) => {
                let out = got.unwrap_or_else(|| panic!("missing treasury output at DAA score {daa}"));
                assert_eq!(out.value, value, "treasury value at DAA score {daa}");
                assert_eq!(out.script_public_key.version(), 0);
                assert_eq!(faster_hex::hex_string(out.script_public_key.script()), v["treasury_script"].as_str().unwrap());
                paying += 1;
            }
        }
    }
    assert_eq!(paying, 8, "vectors inside the two treasury windows");
}

/// The treasury output is the last coinbase output, after the reward outputs, and only inside the windows.
#[test]
fn sedra_expected_coinbase_includes_treasury_output_test() {
    let cbm = mainnet_coinbase_manager();
    let selected_parent = Hash::from_u64_word(7);
    let ghostdag_data = GhostdagData::new_with_selected_parent(selected_parent, 18);
    let miner_spk = ScriptPublicKey::new(0, ScriptVec::from_slice(&[0x20; 34]));
    let mut rewards = BlockHashMap::default();
    rewards.insert(selected_parent, BlockRewardData::new(5_000_000_000, 0, miner_spk.clone()));
    let non_daa = BlockHashSet::default();
    let miner_data = MinerData::new(miner_spk.clone(), vec![]);

    for (daa, expected_treasury) in [
        (53_632_371u64, None),
        (53_632_372, Some(1_000_000_000_000_000u64)),
        (53_632_571, Some(1_000_000_000_000_000)),
        (53_632_572, None),
        (53_633_267, Some(100_000_000_000_000)),
        (53_633_720, Some(100_000_000_000_000)),
        (53_633_721, None),
    ] {
        let template = cbm.expected_coinbase_transaction(daa, miner_data.clone(), &ghostdag_data, &rewards, &non_daa).unwrap();
        let outputs = &template.tx.outputs;
        assert_eq!(outputs[0].value, 5_000_000_000, "blue reward output first at DAA score {daa}");
        match expected_treasury {
            None => assert_eq!(outputs.len(), 1, "no treasury output at DAA score {daa}"),
            Some(value) => {
                assert_eq!(outputs.len(), 2, "treasury output expected at DAA score {daa}");
                let last = outputs.last().unwrap();
                assert_eq!(last.value, value);
                assert_eq!(last.script_public_key.script(), &MAINNET_PARAMS.treasury_script_public_key[..]);
            }
        }
    }
}

/// Dormant on every network except mainnet.
#[test]
fn sedra_relaunch_rules_dormant_outside_mainnet_test() {
    use kaspa_consensus_core::config::params::{DEVNET_PARAMS, SIMNET_PARAMS, TESTNET_PARAMS};
    for p in [TESTNET_PARAMS, SIMNET_PARAMS, DEVNET_PARAMS] {
        assert_eq!(p.difficulty_reset_window_blocks, 0);
        assert!(p.treasury_allocation_windows.is_empty());
        assert!(p.treasury_script_public_key.is_empty());
    }
}
