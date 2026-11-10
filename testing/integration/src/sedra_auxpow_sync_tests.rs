//! SEDRA (M10 step 5b): merged-mined blocks between two nodes.
//!
//! Node A (simnet, real proof of work, merged mining active, small pruning depths) mines a chain of valid blocks,
//! every second one merged-mined: its own nonce fails the target and only the attached proof (a Kaspa parent whose work
//! meets the Sedra target) carries it. Node B receives every block the way a peer sends it, through the P2P header
//! conversion, and must accept all of them. Then B validates A's pruning point proof, which contains merged-mined
//! headers at higher levels: the levels B re-derives must match the ones A stored at acceptance (SIP-0001 §4.7). A node
//! without merged mining rejects both.

use kaspa_consensus::config::ConfigBuilder;
use kaspa_consensus::consensus::test_consensus::TestConsensus;
use kaspa_consensus::params::{ForkActivation, SIMNET_PARAMS};
use kaspa_consensus_core::api::ConsensusApi;
use kaspa_consensus_core::auxpow::AuxPow;
use kaspa_consensus_core::block::{Block, MutableBlock};
use kaspa_consensus_core::coinbase::MinerData;
use kaspa_consensus_core::config::Config;
use kaspa_consensus_core::errors::block::RuleError;
use kaspa_consensus_core::header::Header;
use kaspa_consensus_core::kaspa_parent::{merkle_root_from_coinbase, KaspaParentCoinbase};
use kaspa_consensus_core::pruning::PruningProofMetadata;
use kaspa_consensus_core::subnets::SUBNETWORK_ID_COINBASE;
use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_hashes::Hash;
use kaspa_math::Uint256;
use kaspa_p2p_lib::pb as protowire;

const BLOCKS: usize = 240;

fn config(activation: ForkActivation) -> Config {
    ConfigBuilder::new(SIMNET_PARAMS)
        .edit_consensus_params(|p| {
            p.skip_proof_of_work = false;
            p.aux_pow_activation = activation;
            // small depths so the pruning point moves within a short test chain
            p.prior_merge_depth = 10;
            p.prior_finality_depth = 20;
            p.prior_pruning_depth = 60;
            p.pruning_proof_m = 10;
        })
        .build()
}

fn native_passes(header: &Header) -> bool {
    kaspa_pow::State::new(header).check_pow(header.nonce).0
}

/// Gives a valid template its proof of work: natively (`aux == false`) or through a merged-mining proof.
fn mine(block: &mut MutableBlock, genesis: Hash, aux: bool) {
    let target = Uint256::from_compact_target_bits(block.header.bits);
    if !aux {
        let state = kaspa_pow::State::new(&block.header);
        block.header.nonce = (0u64..).find(|&n| state.check_pow(n).0).unwrap();
        block.header.aux_pow = None;
        block.header.finalize();
        return;
    }

    let commitment = AuxPow::commitment(genesis, AuxPow::nonce_free_header_hash(&block.header));
    let mut payload = b"kaspa pool ".to_vec();
    payload.extend_from_slice(&AuxPow::tagged_commitment(commitment));
    let coinbase = KaspaParentCoinbase { version: 1, outputs: vec![], lock_time: 0, subnetwork_id: SUBNETWORK_ID_COINBASE, gas: 0, payload };
    let root = merkle_root_from_coinbase(coinbase.hash().unwrap(), &[]);
    let mut parent = Header::new_finalized(2, vec![vec![7.into()]], root, 2.into(), 3.into(), block.header.timestamp, 0x1d00ffff, 0, 400_000_000, 9.into(), 399_000_000, 4.into());
    let state = kaspa_pow::State::new(&parent);
    for nonce in 0u64.. {
        // 256 times more work than needed: the parent's level is above the native one, so the proof levels matter
        if state.calculate_pow(nonce) > target >> 8 {
            continue;
        }
        parent.nonce = nonce;
        parent.finalize();
        block.header.nonce = AuxPow::nonce_binding(&parent);
        if !native_passes(&block.header) {
            break;
        }
    }
    block.header.finalize();
    block.header.aux_pow = Some(Box::new(AuxPow { parent_header: parent, parent_coinbase: coinbase, coinbase_merkle_branch: vec![] }));
}

/// The block as a peer relays it: header through the P2P conversion (protobuf and back), transactions as they are.
fn relayed(block: &Block) -> Block {
    let pb = protowire::BlockHeader::from(block.header.as_ref());
    let header = Header::try_from(pb).expect("a relayed header decodes");
    Block::new(header, block.transactions.as_ref().clone())
}

#[tokio::test]
async fn sedra_auxpow_two_node_sync_test() {
    kaspa_alloc::init_allocator_with_default_settings();
    let config_a = config(ForkActivation::always());
    let genesis = config_a.genesis.hash;
    let a = TestConsensus::new(&config_a);
    let b = TestConsensus::new(&config(ForkActivation::always()));
    let legacy = TestConsensus::new(&config(ForkActivation::never()));
    let wait_a = a.init();
    let wait_b = b.init();
    let wait_legacy = legacy.init();

    let miner_data = MinerData::new(ScriptPublicKey::from_vec(0, vec![]), vec![]);
    let mut tip = genesis;
    let mut chain = Vec::with_capacity(BLOCKS);
    for i in 0..BLOCKS {
        let mut block = a.build_utxo_valid_block_with_parents(Hash::from_u64_word(i as u64 + 1), vec![tip], miner_data.clone(), vec![]);
        mine(&mut block, genesis, i % 2 == 1);
        let block = block.to_immutable();
        assert_eq!(block.header.aux_pow.is_some(), i % 2 == 1);
        a.validate_and_insert_block(block.clone()).virtual_state_task.await.unwrap_or_else(|e| panic!("A rejected block {i}: {e}"));
        tip = block.hash();
        chain.push(block);
    }

    // node B accepts every relayed block, merged-mined or not, and keeps the proofs
    for (i, block) in chain.iter().enumerate() {
        let r = relayed(block);
        assert_eq!(r.hash(), block.hash());
        assert_eq!(r.header.aux_pow.is_some(), block.header.aux_pow.is_some());
        b.validate_and_insert_block(r).virtual_state_task.await.unwrap_or_else(|e| panic!("B rejected relayed block {i}: {e}"));
    }
    assert_eq!(b.get_sink(), a.get_sink());

    // a node without merged mining rejects the first merged-mined block
    legacy.validate_and_insert_block(relayed(&chain[0])).virtual_state_task.await.unwrap();
    let result = legacy.validate_and_insert_block(relayed(&chain[1])).virtual_state_task.await;
    assert!(matches!(result, Err(RuleError::InvalidPoW)), "{result:?}");

    // the pruning point moved, and its proof carries merged-mined headers
    let pruning_point = a.pruning_point();
    assert_ne!(pruning_point, genesis, "the pruning point should have moved past genesis");
    let proof = a.get_pruning_point_proof();
    let aux_headers = proof.iter().flatten().filter(|h| h.aux_pow.is_some()).count();
    assert!(aux_headers > 0, "the proof should contain merged-mined headers");
    let levels_with_aux = proof.iter().filter(|level| level.iter().any(|h| h.aux_pow.is_some())).count();
    println!("pruning point DAA {}, proof: {} headers in {} levels, {} merged-mined, {} levels with merged-mined headers", a.get_header(pruning_point).unwrap().daa_score, proof.iter().map(|l| l.len()).sum::<usize>(), proof.len(), aux_headers, levels_with_aux);

    // a fresh node validates it: the levels it re-derives match A's
    let fresh = TestConsensus::new(&config(ForkActivation::always()));
    let wait_fresh = fresh.init();
    let metadata = PruningProofMetadata::new(a.get_header(a.get_sink()).unwrap().blue_work);
    fresh.validate_pruning_proof(&proof, &metadata).unwrap_or_else(|e| panic!("pruning proof rejected: {e}"));

    // without merged mining the same proof is invalid
    let fresh_legacy = TestConsensus::new(&config(ForkActivation::never()));
    let wait_fresh_legacy = fresh_legacy.init();
    let legacy_result = fresh_legacy.validate_pruning_proof(&proof, &metadata);
    println!("without merged mining: {legacy_result:?}");
    assert!(legacy_result.is_err());

    for (tc, wait) in [(a, wait_a), (b, wait_b), (legacy, wait_legacy), (fresh, wait_fresh), (fresh_legacy, wait_fresh_legacy)] {
        tc.shutdown(wait);
    }
}
