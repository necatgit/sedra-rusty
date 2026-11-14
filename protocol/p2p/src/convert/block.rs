use super::{error::ConversionError, option::TryIntoOptionEx};
use crate::pb as protowire;
use kaspa_consensus_core::{block::Block, tx::Transaction};
use prost::Message;

// ----------------------------------------------------------------------------
// consensus_core to protowire
// ----------------------------------------------------------------------------

impl From<&Block> for protowire::BlockMessage {
    fn from(block: &Block) -> Self {
        Self { header: Some(block.header.as_ref().into()), transactions: block.transactions.iter().map(|tx| tx.into()).collect() }
    }
}

// ----------------------------------------------------------------------------
// protowire to consensus_core
// ----------------------------------------------------------------------------

/// SEDRA (security back-port, upstream rusty-kaspa 2a47b249): upper bound for the encoded body (everything except the
/// header) of a block received over P2P, checked before decoding its transactions. At least 2x the largest body the
/// consensus rules allow (see the test below), so it never rejects a valid block.
const MAX_BLOCK_BODY_SIZE: usize = 2 * 1024 * 1024; // 2MB

impl TryFrom<protowire::BlockMessage> for Block {
    type Error = ConversionError;

    fn try_from(block: protowire::BlockMessage) -> Result<Self, Self::Error> {
        if block.encoded_len().saturating_sub(block.header.as_ref().map(|h| h.encoded_len()).unwrap_or(0)) > MAX_BLOCK_BODY_SIZE {
            return Err(ConversionError::Size);
        }
        Ok(Self::new(
            block.header.try_into_ex()?,
            block.transactions.into_iter().map(|i| i.try_into()).collect::<Result<Vec<Transaction>, Self::Error>>()?,
        ))
    }
}

#[cfg(test)]
mod sedra_size_tests {
    use super::*;
    use kaspa_consensus_core::{
        config::params::{DEVNET_PARAMS, MAINNET_PARAMS, SIMNET_PARAMS, TESTNET_PARAMS},
        header::Header,
        subnets::SUBNETWORK_ID_NATIVE,
    };
    use kaspa_hashes::Hash;

    fn block_with_payload(len: usize) -> Block {
        let mut header = Header::from_precomputed_hash(Hash::from_u64_word(1), vec![Hash::from_u64_word(2)]);
        header.finalize(); // the real hash of the header fields, as the receiving side computes it
        let tx = Transaction::new(0, vec![], vec![], 0, SUBNETWORK_ID_NATIVE, 0, vec![7u8; len]);
        Block::new(header, vec![tx])
    }

    #[test]
    fn sedra_oversized_block_message_rejected_test() {
        let message: protowire::BlockMessage = (&block_with_payload(3 * 1024 * 1024)).into();
        assert!(message.encoded_len() > MAX_BLOCK_BODY_SIZE);
        assert!(matches!(Block::try_from(message), Err(ConversionError::Size)));
    }

    #[test]
    fn sedra_normal_block_message_roundtrip_test() {
        let block = block_with_payload(10_000);
        let message: protowire::BlockMessage = (&block).into();
        let back = Block::try_from(message).unwrap();
        assert_eq!(back.header.hash, block.header.hash);
        assert_eq!(back.transactions[0].payload, block.transactions[0].payload);
    }

    /// A transaction's mass is at least its serialized size times `mass_per_tx_byte`, so a valid block body is at most
    /// `max_block_mass / mass_per_tx_byte` bytes. The P2P limit must leave at least 2x slack over that on every network.
    #[test]
    fn sedra_block_size_limit_has_slack_over_consensus_test() {
        for p in [MAINNET_PARAMS, TESTNET_PARAMS, SIMNET_PARAMS, DEVNET_PARAMS] {
            let consensus_max_body_bytes = (p.max_block_mass / p.mass_per_tx_byte.max(1)) as usize;
            assert!(
                MAX_BLOCK_BODY_SIZE >= 2 * consensus_max_body_bytes,
                "{}: P2P limit {} must be at least 2x the consensus limit {}",
                p.net,
                MAX_BLOCK_BODY_SIZE,
                consensus_max_body_bytes
            );
        }
    }
}
