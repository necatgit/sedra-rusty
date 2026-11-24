//! SEDRA (M10 step 4c): merged mining through the real consensus pipeline (header and body processing).
//!
//! Simnet's genesis target is about 2^255, so about half of all hashes meet it. That makes it cheap to find a Kaspa
//! "parent" whose work meets the Sedra target while the Sedra header's own (bound) nonce does not, which is the case
//! where only the merged-mining path can accept the block.

use crate::{config::ConfigBuilder, consensus::test_consensus::TestConsensus, constants::TX_VERSION, errors::RuleError, params::SIMNET_PARAMS};
use kaspa_consensus_core::{
    api::ConsensusApi,
    auxpow::AuxPow,
    block::MutableBlock,
    config::params::ForkActivation,
    header::Header,
    kaspa_parent::{merkle_root_from_coinbase, KaspaParentCoinbase},
    merkle::calc_hash_merkle_root,
    subnets::SUBNETWORK_ID_COINBASE,
};
use crate::model::stores::headers::HeaderStoreReader;
use kaspa_core::assert_match;
use kaspa_hashes::Hash;
use kaspa_math::Uint256;

fn consensus(activation: ForkActivation) -> (TestConsensus, Hash) {
    let config = ConfigBuilder::new(SIMNET_PARAMS)
        .edit_consensus_params(|p| {
            p.skip_proof_of_work = false;
            p.aux_pow_activation = activation;
        })
        .build();
    let genesis = config.genesis.hash;
    (TestConsensus::new(&config), genesis)
}

fn native_passes(header: &Header) -> bool {
    kaspa_pow::State::new(header).check_pow(header.nonce).0
}

/// A block on top of genesis whose own nonce fails PoW, and a proof that makes it valid.
fn aux_mined_block(tc: &TestConsensus, genesis: Hash, later_ms: u64) -> (MutableBlock, AuxPow) {
    let mut block = tc.build_block_with_parents_and_transactions(1.into(), vec![genesis], vec![]);
    block.header.timestamp += later_ms;
    let target = Uint256::from_compact_target_bits(block.header.bits);
    let commitment = AuxPow::commitment(genesis, AuxPow::nonce_free_header_hash(&block.header));
    let mut payload = b"kaspa pool ".to_vec();
    payload.extend_from_slice(&AuxPow::tagged_commitment(commitment));
    let coinbase =
        KaspaParentCoinbase { version: 1, outputs: vec![], lock_time: 0, subnetwork_id: SUBNETWORK_ID_COINBASE, gas: 0, payload };
    let root = merkle_root_from_coinbase(coinbase.hash().unwrap(), &[]);
    let mut parent = Header::new_finalized(2, vec![vec![7.into()]], root, 2.into(), 3.into(), 1_794_000_000_000, 0x1d00ffff, 0, 400_000_000, 9.into(), 399_000_000, 4.into());
    let state = kaspa_pow::State::new(&parent);
    for nonce in 0u64.. {
        parent.nonce = nonce;
        // 4096 times more work than needed, so the parent's block level is clearly above the native one
        if state.calculate_pow(nonce) > target >> 12 {
            continue;
        }
        block.header.nonce = AuxPow::nonce_binding(&parent);
        if !native_passes(&block.header) {
            break;
        }
    }
    parent.finalize();
    block.header.finalize();
    (block, AuxPow { parent_header: parent, parent_coinbase: coinbase, coinbase_merkle_branch: vec![] })
}

/// A natively mined block on top of genesis whose coinbase carries an SDM1 commitment.
fn native_block_with_commitment(tc: &TestConsensus, genesis: Hash) -> MutableBlock {
    let mut block = tc.build_block_with_parents_and_transactions(2.into(), vec![genesis], vec![]);
    block.transactions[0].payload.extend_from_slice(&AuxPow::tagged_commitment(Hash::from_u64_word(5)));
    assert_eq!(block.transactions[0].version, TX_VERSION);
    block.header.hash_merkle_root = calc_hash_merkle_root(block.transactions.iter(), false);
    let state = kaspa_pow::State::new(&block.header);
    block.header.nonce = (0u64..).find(|&n| state.check_pow(n).0).unwrap();
    block.header.finalize();
    block
}

#[tokio::test]
async fn sedra_auxpow_consensus_accepts_proof_after_activation_test() {
    let (tc, genesis) = consensus(ForkActivation::always());
    let wait = tc.init();
    let (block, aux) = aux_mined_block(&tc, genesis, 0);
    let hash = block.header.hash;

    // without the proof (stripped in transit) the block fails PoW ...
    assert_match!(tc.validate_and_insert_block(block.clone().to_immutable()).block_task.await, Err(RuleError::InvalidPoW));
    // ... and is not cached as invalid: with the proof it is accepted
    let mut with = block.clone();
    with.header.aux_pow = Some(Box::new(aux.clone()));
    assert_eq!(with.header.hash, hash);
    tc.validate_and_insert_block(with.to_immutable()).block_task.await.unwrap();
    let stored = tc.headers_store().get_header(hash).unwrap();
    assert!(stored.aux_pow.is_some(), "the header store keeps the proof");

    // a proof for another block does not help
    let (other, _) = aux_mined_block(&tc, genesis, 1);
    assert_ne!(other.header.hash, hash);
    let mut wrong = other.clone();
    wrong.header.aux_pow = Some(Box::new(aux));
    assert_match!(tc.validate_and_insert_block(wrong.to_immutable()).block_task.await, Err(RuleError::InvalidPoW));
    tc.shutdown(wait);
}

#[tokio::test]
async fn sedra_auxpow_consensus_ignores_proof_before_activation_test() {
    let (tc, genesis) = consensus(ForkActivation::never());
    let wait = tc.init();
    let (mut block, aux) = aux_mined_block(&tc, genesis, 0);
    block.header.aux_pow = Some(Box::new(aux));
    assert_match!(tc.validate_and_insert_block(block.to_immutable()).block_task.await, Err(RuleError::InvalidPoW));

    // and the coinbase ban is not active either
    tc.validate_and_insert_block(native_block_with_commitment(&tc, genesis).to_immutable()).block_task.await.unwrap();
    tc.shutdown(wait);
}

#[tokio::test]
async fn sedra_auxpow_consensus_coinbase_ban_test() {
    let (tc, genesis) = consensus(ForkActivation::always());
    let wait = tc.init();
    let block = native_block_with_commitment(&tc, genesis);
    assert_match!(
        tc.validate_and_insert_block(block.to_immutable()).block_task.await,
        Err(RuleError::CoinbaseCarriesMergeMiningCommitment)
    );
    tc.shutdown(wait);
}

#[test]
fn sedra_auxpow_ban_window_test() {
    let finality = SIMNET_PARAMS.finality_depth().upper_bound();
    let mut p = SIMNET_PARAMS;
    assert_eq!(p.coinbase_commitment_ban_activation(), ForkActivation::never(), "never stays never");
    p.aux_pow_activation = ForkActivation::new(10 * finality);
    assert_eq!(p.coinbase_commitment_ban_activation(), ForkActivation::new(8 * finality));
    p.aux_pow_activation = ForkActivation::new(finality);
    assert_eq!(p.coinbase_commitment_ban_activation(), ForkActivation::always(), "saturates at always");
    for params in [crate::params::MAINNET_PARAMS, SIMNET_PARAMS, crate::params::DEVNET_PARAMS] {
        assert_eq!(params.aux_pow_activation, ForkActivation::never(), "not scheduled outside the testnet");
    }
}

/// SEDRA (#13): the activation written into the testnet release (SIP-0003; sedra-project testnet/RUNBOOK.md 3.1).
#[test]
fn sedra_auxpow_testnet_release_activation_test() {
    let p = crate::params::TESTNET_PARAMS;
    assert_eq!(p.finality_depth().upper_bound(), 86_400);
    assert_eq!(p.genesis.daa_score, 0, "a fresh chain from genesis starts at DAA score 0");
    assert_eq!(p.aux_pow_activation, ForkActivation::new(345_600), "4 x 86,400: about day 4 at one block per second");
    assert_eq!(p.coinbase_commitment_ban_activation(), ForkActivation::new(172_800), "2 x finality depth earlier: about day 2");
    assert!(!p.aux_pow_activation.is_active(345_599) && p.aux_pow_activation.is_active(345_600));
    let ban = p.coinbase_commitment_ban_activation();
    assert!(!ban.is_active(172_799) && ban.is_active(172_800));
    // mainnet keeps the rules of the mainnet releases
    assert_eq!(crate::params::MAINNET_PARAMS.coinbase_commitment_ban_activation(), ForkActivation::never());
}

/// SEDRA (#13): the activation boundary through the real pipeline, at the exact DAA score of the block (activation one
/// above it: the proof is ignored; activation at it: accepted). The release's own scores are checked above; a chain
/// cannot be brought to DAA 345,600 in a unit test, so the same rule is shown at the score of a block on genesis (the
/// test builder gives it DAA score 0, so "one above" is activation 1 and "at it" is activation 0).
#[tokio::test]
async fn sedra_auxpow_activation_boundary_test() {
    for (offset, accepted) in [(1u64, false), (0, true)] {
        let (probe, probe_genesis) = consensus(ForkActivation::never());
        let probe_wait = probe.init();
        let daa = aux_mined_block(&probe, probe_genesis, 0).0.header.daa_score;
        probe.shutdown(probe_wait);
        assert_eq!(daa, 0, "the score the comment above relies on");
        let (tc, genesis) = consensus(ForkActivation::new(daa + offset));
        let wait = tc.init();
        let (mut block, aux) = aux_mined_block(&tc, genesis, 0);
        assert_eq!(block.header.daa_score, daa);
        block.header.aux_pow = Some(Box::new(aux));
        let result = tc.validate_and_insert_block(block.to_immutable()).block_task.await;
        if accepted {
            result.unwrap();
        } else {
            assert_match!(result, Err(RuleError::InvalidPoW));
        }
        tc.shutdown(wait);
    }
}

/// SEDRA (#13): the self-parent ban boundary: an `SDM1` commitment in a Sedra coinbase is accepted when the ban starts
/// one DAA score above the block, and rejected when it starts at the block's score.
#[tokio::test]
async fn sedra_auxpow_ban_boundary_test() {
    let two_finality = 2 * SIMNET_PARAMS.finality_depth().upper_bound();
    for (offset, accepted) in [(1u64, true), (0, false)] {
        let (probe, probe_genesis) = consensus(ForkActivation::never());
        let probe_wait = probe.init();
        let daa = native_block_with_commitment(&probe, probe_genesis).header.daa_score;
        probe.shutdown(probe_wait);
        let activation = ForkActivation::new(daa + offset + two_finality);
        let mut p = SIMNET_PARAMS;
        p.aux_pow_activation = activation;
        assert_eq!(p.coinbase_commitment_ban_activation(), ForkActivation::new(daa + offset));
        let (tc, genesis) = consensus(activation);
        let wait = tc.init();
        let block = native_block_with_commitment(&tc, genesis);
        assert_eq!(block.header.daa_score, daa);
        let result = tc.validate_and_insert_block(block.to_immutable()).block_task.await;
        if accepted {
            result.unwrap();
        } else {
            assert_match!(result, Err(RuleError::CoinbaseCarriesMergeMiningCommitment));
        }
        tc.shutdown(wait);
    }
}

#[tokio::test]
async fn sedra_auxpow_pruning_proof_level_test() {
    // SIP-0001 §4.7: pruning-proof code re-derives block levels; it must reproduce the level stored at acceptance
    for (activation, aux_counts) in [(ForkActivation::always(), true), (ForkActivation::never(), false)] {
        let (tc, genesis) = consensus(activation);
        let wait = tc.init();
        let (block, aux) = aux_mined_block(&tc, genesis, 0);
        let max = tc.params().max_block_level;
        let parent_level = kaspa_pow::calc_level_from_pow(kaspa_pow::auxpow::parent_pow(&aux), max);
        let native_level = kaspa_pow::calc_block_level(&block.header, max);
        assert_ne!(parent_level, native_level, "the test needs the two levels to differ");
        let mut with = block.header.clone();
        with.aux_pow = Some(Box::new(aux));
        let expected = if aux_counts { parent_level } else { native_level };
        assert_eq!(tc.pruning_proof_manager().calc_block_level(&with), expected, "activation {activation:?}");
        assert_eq!(tc.pruning_proof_manager().calc_block_level(&block.header), native_level);
        tc.shutdown(wait);
    }
}
