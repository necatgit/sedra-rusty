use crate::{auxpow::AuxPow, hashing, BlueWorkType};
use borsh::{BorshDeserialize, BorshSerialize};
use kaspa_hashes::Hash;
use kaspa_utils::mem_size::MemSizeEstimator;
use serde::{Deserialize, Serialize};

/// @category Consensus
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// Cached hash
    pub hash: Hash,
    pub version: u16,
    pub parents_by_level: Vec<Vec<Hash>>,
    pub hash_merkle_root: Hash,
    pub accepted_id_merkle_root: Hash,
    pub utxo_commitment: Hash,
    /// Timestamp is in milliseconds
    pub timestamp: u64,
    pub bits: u32,
    pub nonce: u64,
    pub daa_score: u64,
    pub blue_work: BlueWorkType,
    pub blue_score: u64,
    pub pruning_point: Hash,
    /// SEDRA (M10): optional merged-mining proof (SIP-0001 §4.1). **Not part of the header hash**
    /// (`hashing::header::hash` writes the fields above one by one), so the block hash is the same whether the block
    /// was mined natively or through a Kaspa parent, and a relaying peer can strip the proof without changing the
    /// block's identity. Only the proof-of-work gate reads it, and only after activation. `None` for native blocks.
    ///
    /// Inputs that omit the field (RPC JSON) decode to `None` (serde treats a missing `Option` that way). No
    /// `skip_serializing_if`: bincode (the header store, not self-describing) must always write it. Adding the field
    /// changed the header store format (DB version 5).
    pub aux_pow: Option<Box<AuxPow>>,
}

impl Header {
    #[allow(clippy::too_many_arguments)]
    pub fn new_finalized(
        version: u16,
        parents_by_level: Vec<Vec<Hash>>,
        hash_merkle_root: Hash,
        accepted_id_merkle_root: Hash,
        utxo_commitment: Hash,
        timestamp: u64,
        bits: u32,
        nonce: u64,
        daa_score: u64,
        blue_work: BlueWorkType,
        blue_score: u64,
        pruning_point: Hash,
    ) -> Self {
        let mut header = Self {
            hash: Default::default(), // Temp init before the finalize below
            version,
            parents_by_level,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
            nonce,
            timestamp,
            daa_score,
            bits,
            blue_work,
            blue_score,
            pruning_point,
            aux_pow: None,
        };
        header.finalize();
        header
    }

    /// Attaches a merged-mining proof. The hash is unaffected, so no re-finalize is needed.
    pub fn with_aux_pow(mut self, aux_pow: AuxPow) -> Self {
        self.aux_pow = Some(Box::new(aux_pow));
        self
    }

    /// Finalizes the header and recomputes the header hash
    pub fn finalize(&mut self) {
        self.hash = hashing::header::hash(self);
    }

    pub fn direct_parents(&self) -> &[Hash] {
        if self.parents_by_level.is_empty() {
            &[]
        } else {
            &self.parents_by_level[0]
        }
    }

    /// WARNING: To be used for test purposes only
    pub fn from_precomputed_hash(hash: Hash, parents: Vec<Hash>) -> Header {
        Header {
            version: crate::constants::BLOCK_VERSION,
            hash,
            parents_by_level: vec![parents],
            hash_merkle_root: Default::default(),
            accepted_id_merkle_root: Default::default(),
            utxo_commitment: Default::default(),
            nonce: 0,
            timestamp: 0,
            daa_score: 0,
            bits: 0,
            blue_work: 0.into(),
            blue_score: 0,
            pruning_point: Default::default(),
            aux_pow: None,
        }
    }
}

impl AsRef<Header> for Header {
    fn as_ref(&self) -> &Header {
        self
    }
}

impl MemSizeEstimator for Header {
    fn estimate_mem_bytes(&self) -> usize {
        size_of::<Self>()
            + self.parents_by_level.iter().map(|l| l.len()).sum::<usize>() * size_of::<Hash>()
            // SEDRA (M10): the proof lives on the heap and must count against the header cache budget
            + self.aux_pow.as_ref().map_or(0, |a| a.estimate_mem_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaspa_math::Uint192;
    use serde_json::Value;

    #[test]
    fn test_header_ser() {
        let header = Header::new_finalized(
            1,
            vec![vec![1.into()]],
            Default::default(),
            Default::default(),
            Default::default(),
            234,
            23,
            567,
            0,
            Uint192([0x1234567890abcfed, 0xc0dec0ffeec0ffee, 0x1234567890abcdef]),
            u64::MAX,
            Default::default(),
        );
        let json = serde_json::to_string(&header).unwrap();
        println!("{}", json);

        let v = serde_json::from_str::<Value>(&json).unwrap();
        let blue_work = v.get("blueWork").expect("missing `blueWork` property");
        let blue_work = blue_work.as_str().expect("`blueWork` is not a string");
        assert_eq!(blue_work, "1234567890abcdefc0dec0ffeec0ffee1234567890abcfed");
        let blue_score = v.get("blueScore").expect("missing `blueScore` property");
        let blue_score: u64 = blue_score.as_u64().expect("blueScore is not a u64 compatible value");
        assert_eq!(blue_score, u64::MAX);

        let h = serde_json::from_str::<Header>(&json).unwrap();
        assert!(h.blue_score == header.blue_score && h.blue_work == header.blue_work);
    }
}
