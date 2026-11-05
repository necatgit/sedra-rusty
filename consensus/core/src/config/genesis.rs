use crate::{block::Block, header::Header, subnets::SUBNETWORK_ID_COINBASE, tx::Transaction};
use kaspa_hashes::{Hash, ZERO_HASH};
use kaspa_muhash::EMPTY_MUHASH;

/// The constants uniquely representing the genesis block
#[derive(Clone, Debug)]
pub struct GenesisBlock {
    pub hash: Hash,
    pub version: u16,
    pub hash_merkle_root: Hash,
    pub utxo_commitment: Hash,
    pub timestamp: u64,
    pub bits: u32,
    pub nonce: u64,
    pub daa_score: u64,
    pub coinbase_payload: &'static [u8],
}

impl GenesisBlock {
    pub fn build_genesis_transactions(&self) -> Vec<Transaction> {
        vec![Transaction::new(0, Vec::new(), Vec::new(), 0, SUBNETWORK_ID_COINBASE, 0, self.coinbase_payload.to_vec())]
    }
}

impl From<&GenesisBlock> for Header {
    fn from(genesis: &GenesisBlock) -> Self {
        Header::new_finalized(
            genesis.version,
            Vec::new(),
            genesis.hash_merkle_root,
            ZERO_HASH,
            genesis.utxo_commitment,
            genesis.timestamp,
            genesis.bits,
            genesis.nonce,
            genesis.daa_score,
            0.into(),
            0,
            ZERO_HASH,
        )
    }
}

impl From<&GenesisBlock> for Block {
    fn from(genesis: &GenesisBlock) -> Self {
        Block::new(genesis.into(), genesis.build_genesis_transactions())
    }
}

impl From<(&Header, &'static [u8])> for GenesisBlock {
    fn from((header, payload): (&Header, &'static [u8])) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            hash_merkle_root: header.hash_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            coinbase_payload: payload,
        }
    }
}

/// The genesis block of the Sedra mainnet. Values copied from sedrad v2.0.0 (`domain/dagconfig/genesis.go`).
/// Unlike Kaspa (checkpoint genesis at DAA score 1312860), Sedra starts at DAA score 0 with an empty UTXO set.
pub const GENESIS: GenesisBlock = GenesisBlock {
    hash: Hash::from_bytes([0x7c, 0x38, 0x0a, 0xe6, 0x90, 0xf9, 0xea, 0xa6, 0x4f, 0x30, 0x04, 0x51, 0xd5, 0xe4, 0x66, 0x37, 0x11, 0x13, 0xc3, 0x8b, 0x96, 0xed, 0x96, 0xfe, 0x3f, 0x1e, 0x93, 0x48, 0x69, 0x35, 0x5f, 0xfa]),
    version: 0,
    hash_merkle_root: Hash::from_bytes([0x0a, 0x65, 0x43, 0x55, 0xc1, 0xd5, 0x8b, 0xd4, 0xab, 0xd9, 0xbd, 0xcc, 0x2e, 0x52, 0xf1, 0xbd, 0x56, 0x58, 0x64, 0xe8, 0xa1, 0x6c, 0xe0, 0x37, 0x7b, 0x48, 0xec, 0xc6, 0x4d, 0xb5, 0x4b, 0x27]),
    utxo_commitment: Hash::from_bytes([0x54, 0x4e, 0xb3, 0x14, 0x2c, 0x00, 0x0f, 0x0a, 0xd2, 0xc7, 0x6a, 0xc4, 0x1f, 0x42, 0x22, 0xab, 0xba, 0xba, 0xbe, 0xd8, 0x30, 0xee, 0xaf, 0xee, 0x4b, 0x6d, 0xc5, 0x6b, 0x52, 0xd5, 0xca, 0xc0]), // empty MuHash
    timestamp: 0x14d2bd72185,
    bits: 525264379,
    nonce: 0x1a14e,
    daa_score: 0,
    #[rustfmt::skip]
    coinbase_payload: &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Blue score
        0x00, 0xE1, 0xF5, 0x05, 0x00, 0x00, 0x00, 0x00, // Subsidy
        0x00, 0x00,                                     // Script version
        0x01,                                           // Varint
        0x00,                                           // OP-FALSE
        0x73, 0x65, 0x64, 0x72, 0x61, 0x2D, 0x6D, 0x61, 0x69, 0x6E, 0x6E, 0x65, 0x74, // "sedra-mainnet"
    ],
};

pub const TESTNET_GENESIS: GenesisBlock = GenesisBlock {
    hash: Hash::from_bytes([
        0xf8, 0x96, 0xa3, 0x03, 0x48, 0x73, 0xbe, 0x17, 0x39, 0xfc, 0x43, 0x59, 0x23, 0x68, 0x99, 0xfd, 0x3d, 0x65, 0xd2, 0xbc, 0x94,
        0xf9, 0x78, 0x0d, 0xf0, 0xd0, 0xda, 0x3e, 0xb1, 0xcc, 0x43, 0x70,
    ]),
    version: 0,
    hash_merkle_root: Hash::from_bytes([
        0x17, 0x34, 0x14, 0x08, 0xa5, 0x72, 0x45, 0x56, 0x50, 0x4d, 0xf4, 0xd6, 0xcf, 0x51, 0x5c, 0xbf, 0xbb, 0x22, 0x04, 0x30, 0xdc,
        0x45, 0x1c, 0x74, 0x3c, 0x22, 0xd5, 0xe9, 0x11, 0x72, 0x0c, 0x2a,
    ]),
    utxo_commitment: EMPTY_MUHASH,
    timestamp: 0x17c5f62fbb6,
    bits: 0x1e7fffff,
    nonce: 0x14582,
    daa_score: 0,
    #[rustfmt::skip]
    coinbase_payload: &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Blue score
        0x00, 0xE1, 0xF5, 0x05, 0x00, 0x00, 0x00, 0x00, // Subsidy
        0x00, 0x00, // Script version
        0x01,                                                                         // Varint
        0x00,                                                                         // OP-FALSE
        0x6b, 0x61, 0x73, 0x70, 0x61, 0x2d, 0x74, 0x65, 0x73, 0x74, 0x6e, 0x65, 0x74, // kaspa-testnet
    ],
};

pub const TESTNET11_GENESIS: GenesisBlock = GenesisBlock {
    hash: Hash::from_bytes([
        0xcf, 0xa2, 0xa7, 0xeb, 0xfb, 0x8b, 0x4e, 0xa3, 0x45, 0x30, 0x7b, 0xc2, 0x5e, 0xf9, 0x42, 0x1b, 0x23, 0x91, 0xf0, 0x9c, 0x8b,
        0x2c, 0xf2, 0x15, 0xf0, 0x75, 0x60, 0xaf, 0x0d, 0x4d, 0x71, 0x64,
    ]),
    hash_merkle_root: Hash::from_bytes([
        0x3c, 0x35, 0xdb, 0x98, 0x02, 0x7e, 0x84, 0x6e, 0x02, 0xcc, 0x60, 0xb7, 0xa7, 0xfa, 0xb1, 0x6d, 0x6c, 0xf2, 0xa5, 0x42, 0xd8,
        0xe1, 0x60, 0xad, 0x9c, 0xd9, 0x08, 0x5f, 0x51, 0x0c, 0x47, 0xbb,
    ]),
    bits: 504155340, // see `gen_testnet11_genesis`
    #[rustfmt::skip]
    coinbase_payload: &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Blue score
        0x00, 0xE1, 0xF5, 0x05, 0x00, 0x00, 0x00, 0x00, // Subsidy
        0x00, 0x00, // Script version
        0x01,                                                                         // Varint
        0x00,                                                                         // OP-FALSE
        0x6b, 0x61, 0x73, 0x70, 0x61, 0x2d, 0x74, 0x65, 0x73, 0x74, 0x6e, 0x65, 0x74, // kaspa-testnet
        11, 4                                                                         // TN11, Relaunch 4
    ],
    ..TESTNET_GENESIS
};

pub const SIMNET_GENESIS: GenesisBlock = GenesisBlock {
    hash: Hash::from_bytes([
        0x41, 0x1f, 0x8c, 0xd2, 0x6f, 0x3d, 0x41, 0xae, 0xa3, 0x9e, 0x78, 0x57, 0x39, 0x27, 0xda, 0x24, 0xd2, 0x39, 0x95, 0x70, 0x5b,
        0x57, 0x9f, 0x30, 0x95, 0x9b, 0x91, 0x27, 0xe9, 0x6b, 0x79, 0xe3,
    ]),
    version: 0,
    hash_merkle_root: Hash::from_bytes([
        0x19, 0x46, 0xd6, 0x29, 0xf7, 0xe9, 0x22, 0xa7, 0xbc, 0xed, 0x59, 0x19, 0x05, 0x21, 0xc3, 0x77, 0x1f, 0x73, 0xd3, 0x52, 0xdd,
        0xbb, 0xb6, 0x86, 0x56, 0x4a, 0xd7, 0xfd, 0x56, 0x85, 0x7c, 0x1b,
    ]),
    utxo_commitment: EMPTY_MUHASH,
    timestamp: 0x17c5f62fbb6,
    bits: 0x207fffff,
    nonce: 0x2,
    daa_score: 0,
    #[rustfmt::skip]
    coinbase_payload: &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Blue score
        0x00, 0xE1, 0xF5, 0x05, 0x00, 0x00, 0x00, 0x00, // Subsidy
        0x00, 0x00, // Script version
        0x01,                                                                   // Varint
        0x00,                                                                   // OP-FALSE
        0x6b, 0x61, 0x73, 0x70, 0x61, 0x2d, 0x73, 0x69, 0x6d, 0x6e, 0x65, 0x74, // kaspa-simnet
    ],
};

/// Sedra's devnet genesis is identical to its mainnet genesis (sedrad v2.0.0 `genesis.go`).
pub const DEVNET_GENESIS: GenesisBlock = GENESIS;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::bps::TenBps, merkle::calc_hash_merkle_root};

    /// SEDRA: the mainnet genesis computed from its fields must equal the genesis hash in sedrad v2.0.0
    /// (`domain/dagconfig/genesis.go`) and on the live network. Both the header hash and the merkle root are recomputed.
    #[test]
    fn test_sedra_mainnet_genesis() {
        let block: Block = (&GENESIS).into();
        assert_eq!(block.hash().to_string(), "7c380ae690f9eaa64f300451d5e466371113c38b96ed96fe3f1e934869355ffa");
        assert_eq!(
            calc_hash_merkle_root(block.transactions.iter(), false).to_string(),
            "0a654355c1d58bd4abd9bdcc2e52f1bd565864e8a16ce0377b48ecc64db54b27"
        );
        assert_eq!(DEVNET_GENESIS.hash, GENESIS.hash);
    }

    #[test]
    fn test_genesis_hashes() {
        [GENESIS, TESTNET_GENESIS, TESTNET11_GENESIS, SIMNET_GENESIS, DEVNET_GENESIS].into_iter().for_each(|genesis| {
            let block: Block = (&genesis).into();
            assert_hashes_eq(calc_hash_merkle_root(block.transactions.iter(), false), block.header.hash_merkle_root);
            assert_hashes_eq(block.hash(), genesis.hash);
        });
    }

    #[test]
    fn gen_testnet11_genesis() {
        let bps = TenBps::bps();
        let mut genesis = TESTNET_GENESIS;
        let target = kaspa_math::Uint256::from_compact_target_bits(genesis.bits);
        let scaled_target = target * bps / 100;
        let scaled_bits = scaled_target.compact_target_bits();
        genesis.bits = scaled_bits;
        if genesis.bits != TESTNET11_GENESIS.bits {
            panic!("Testnet 11: new bits: {}\nnew hash: {:#04x?}", scaled_bits, Block::from(&genesis).hash().as_bytes());
        }
    }

    fn assert_hashes_eq(got: Hash, expected: Hash) {
        if got != expected {
            // Special hex print to ease changing the genesis hash according to the print if needed
            panic!("Got hash {:#04x?} while expecting {:#04x?}", got.as_bytes(), expected.as_bytes());
        }
    }
}
