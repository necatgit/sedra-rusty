use borsh::{BorshDeserialize, BorshSerialize};
use crate::{FromRpcHex, RpcError, RpcResult, ToRpcHex};
use kaspa_consensus_core::{
    auxpow::{AuxPow, MAX_AUX_POW_SERIALIZED_LEN},
    header::Header,
    BlueWorkType,
};
use kaspa_hashes::Hash;
use serde::{Deserialize, Serialize};
use workflow_serializer::prelude::*;

/// Raw Rpc header type - without a cached header hash.
/// Used for mining APIs (get_block_template & submit_block)
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcRawHeader {
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
    /// SEDRA (M10): hex of the borsh-encoded merged-mining proof, empty for a native block. Not part of the block
    /// hash. Lets a pool submit a merged-mined block through the normal `submit_block`. `serde(default)` so JSON
    /// clients that do not know the field keep working.
    #[serde(default)]
    pub aux_pow: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcHeader {
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
}

impl RpcHeader {
    pub fn direct_parents(&self) -> &[Hash] {
        if self.parents_by_level.is_empty() {
            &[]
        } else {
            &self.parents_by_level[0]
        }
    }
}

impl AsRef<RpcHeader> for RpcHeader {
    fn as_ref(&self) -> &RpcHeader {
        self
    }
}

impl From<Header> for RpcHeader {
    fn from(header: Header) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            parents_by_level: header.parents_by_level,
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
        }
    }
}

impl From<&Header> for RpcHeader {
    fn from(header: &Header) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            parents_by_level: header.parents_by_level.clone(),
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
        }
    }
}

impl From<RpcHeader> for Header {
    fn from(header: RpcHeader) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            parents_by_level: header.parents_by_level,
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
            aux_pow: None, // SEDRA (M10): the query header carries no proof (as in ZKas)
        }
    }
}

impl From<&RpcHeader> for Header {
    fn from(header: &RpcHeader) -> Self {
        Self {
            hash: header.hash,
            version: header.version,
            parents_by_level: header.parents_by_level.clone(),
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
            aux_pow: None, // SEDRA (M10): the query header carries no proof (as in ZKas)
        }
    }
}

impl Serializer for RpcHeader {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        store!(u16, &1, writer)?;

        store!(Hash, &self.hash, writer)?;
        store!(u16, &self.version, writer)?;
        store!(Vec<Vec<Hash>>, &self.parents_by_level, writer)?;
        store!(Hash, &self.hash_merkle_root, writer)?;
        store!(Hash, &self.accepted_id_merkle_root, writer)?;
        store!(Hash, &self.utxo_commitment, writer)?;
        store!(u64, &self.timestamp, writer)?;
        store!(u32, &self.bits, writer)?;
        store!(u64, &self.nonce, writer)?;
        store!(u64, &self.daa_score, writer)?;
        store!(BlueWorkType, &self.blue_work, writer)?;
        store!(u64, &self.blue_score, writer)?;
        store!(Hash, &self.pruning_point, writer)?;

        Ok(())
    }
}

impl Deserializer for RpcHeader {
    fn deserialize<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let _version = load!(u16, reader)?;

        let hash = load!(Hash, reader)?;
        let version = load!(u16, reader)?;
        let parents_by_level = load!(Vec<Vec<Hash>>, reader)?;
        let hash_merkle_root = load!(Hash, reader)?;
        let accepted_id_merkle_root = load!(Hash, reader)?;
        let utxo_commitment = load!(Hash, reader)?;
        let timestamp = load!(u64, reader)?;
        let bits = load!(u32, reader)?;
        let nonce = load!(u64, reader)?;
        let daa_score = load!(u64, reader)?;
        let blue_work = load!(BlueWorkType, reader)?;
        let blue_score = load!(u64, reader)?;
        let pruning_point = load!(Hash, reader)?;

        Ok(Self {
            hash,
            version,
            parents_by_level,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
            timestamp,
            bits,
            nonce,
            daa_score,
            blue_work,
            blue_score,
            pruning_point,
        })
    }
}

impl TryFrom<RpcRawHeader> for Header {
    type Error = RpcError;

    fn try_from(header: RpcRawHeader) -> RpcResult<Self> {
        let built = Self::new_finalized(
            header.version,
            header.parents_by_level,
            header.hash_merkle_root,
            header.accepted_id_merkle_root,
            header.utxo_commitment,
            header.timestamp,
            header.bits,
            header.nonce,
            header.daa_score,
            header.blue_work,
            header.blue_score,
            header.pruning_point,
        );
        attach_aux_pow(built, &header.aux_pow)
    }
}

impl TryFrom<&RpcRawHeader> for Header {
    type Error = RpcError;

    fn try_from(header: &RpcRawHeader) -> RpcResult<Self> {
        let built = Self::new_finalized(
            header.version,
            header.parents_by_level.clone(),
            header.hash_merkle_root,
            header.accepted_id_merkle_root,
            header.utxo_commitment,
            header.timestamp,
            header.bits,
            header.nonce,
            header.daa_score,
            header.blue_work,
            header.blue_score,
            header.pruning_point,
        );
        attach_aux_pow(built, &header.aux_pow)
    }
}

impl From<&Header> for RpcRawHeader {
    fn from(header: &Header) -> Self {
        Self {
            version: header.version,
            parents_by_level: header.parents_by_level.clone(),
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            aux_pow: aux_pow_to_hex(&header),
            pruning_point: header.pruning_point,
        }
    }
}

impl From<Header> for RpcRawHeader {
    fn from(header: Header) -> Self {
        Self {
            aux_pow: aux_pow_to_hex(&header), // before the fields below move out of `header`
            version: header.version,
            parents_by_level: header.parents_by_level,
            hash_merkle_root: header.hash_merkle_root,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            blue_score: header.blue_score,
            pruning_point: header.pruning_point,
        }
    }
}

impl Serializer for RpcRawHeader {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // SEDRA (M10): format version 2 adds the merged-mining proof at the end
        store!(u16, &2, writer)?;

        store!(u16, &self.version, writer)?;
        store!(Vec<Vec<Hash>>, &self.parents_by_level, writer)?;
        store!(Hash, &self.hash_merkle_root, writer)?;
        store!(Hash, &self.accepted_id_merkle_root, writer)?;
        store!(Hash, &self.utxo_commitment, writer)?;
        store!(u64, &self.timestamp, writer)?;
        store!(u32, &self.bits, writer)?;
        store!(u64, &self.nonce, writer)?;
        store!(u64, &self.daa_score, writer)?;
        store!(BlueWorkType, &self.blue_work, writer)?;
        store!(u64, &self.blue_score, writer)?;
        store!(Hash, &self.pruning_point, writer)?;
        store!(String, &self.aux_pow, writer)?;

        Ok(())
    }
}

impl Deserializer for RpcRawHeader {
    fn deserialize<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let format_version = load!(u16, reader)?;

        let version = load!(u16, reader)?;
        let parents_by_level = load!(Vec<Vec<Hash>>, reader)?;
        let hash_merkle_root = load!(Hash, reader)?;
        let accepted_id_merkle_root = load!(Hash, reader)?;
        let utxo_commitment = load!(Hash, reader)?;
        let timestamp = load!(u64, reader)?;
        let bits = load!(u32, reader)?;
        let nonce = load!(u64, reader)?;
        let daa_score = load!(u64, reader)?;
        let blue_work = load!(BlueWorkType, reader)?;
        let blue_score = load!(u64, reader)?;
        let pruning_point = load!(Hash, reader)?;
        let aux_pow = if format_version >= 2 { load!(String, reader)? } else { String::new() };

        Ok(Self {
            version,
            parents_by_level,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
            timestamp,
            bits,
            nonce,
            daa_score,
            blue_work,
            blue_score,
            pruning_point,
            aux_pow,
        })
    }
}

/// SEDRA (M10): the proof as RPC hex (empty for a native block).
fn aux_pow_to_hex(header: &Header) -> String {
    header.aux_pow.as_deref().map(|a| borsh::to_vec(a).expect("AuxPow is always borsh-serializable").to_rpc_hex()).unwrap_or_default()
}

/// SEDRA (M10): attaches a submitted proof. Unlike P2P, a bad proof is an error here, so the submitting pool sees why
/// instead of a bare proof-of-work failure.
fn attach_aux_pow(header: Header, aux_pow_hex: &str) -> RpcResult<Header> {
    if aux_pow_hex.is_empty() {
        return Ok(header);
    }
    if aux_pow_hex.len() > 2 * MAX_AUX_POW_SERIALIZED_LEN {
        return Err(RpcError::General(format!("auxPow too large: {} hex chars", aux_pow_hex.len())));
    }
    let bytes = Vec::<u8>::from_rpc_hex(aux_pow_hex).map_err(|e| RpcError::General(format!("invalid auxPow hex: {e}")))?;
    let aux: AuxPow = borsh::from_slice(&bytes).map_err(|e| RpcError::General(format!("invalid auxPow bytes: {e}")))?;
    aux.validate_structure().map_err(|reason| RpcError::General(format!("invalid auxPow: {reason}")))?;
    Ok(header.with_aux_pow(aux))
}

#[cfg(test)]
mod sedra_aux_tests {
    use super::*;
    use kaspa_consensus_core::{kaspa_parent::KaspaParentCoinbase, subnets::SUBNETWORK_ID_COINBASE};

    fn sample_header() -> Header {
        Header::new_finalized(1, vec![vec![7.into()]], 1.into(), 2.into(), 3.into(), 1_794_000_000_000, 0x1f0fffff, 42, 61_500_000, 9.into(), 61_000_000, 4.into())
    }

    fn sample_aux() -> AuxPow {
        let parent = Header::new_finalized(2, vec![vec![11.into(), 12.into()], vec![13.into()]], 5.into(), 6.into(), 7.into(), 1_794_000_000_500, 0x1d00ffff, 99, 400_000_000, 8.into(), 399_000_000, 10.into());
        let coinbase = KaspaParentCoinbase {
            version: 1,
            outputs: vec![],
            lock_time: 0,
            subnetwork_id: SUBNETWORK_ID_COINBASE,
            gas: 0,
            payload: b"pool SDM1xxxx".to_vec(),
        };
        AuxPow { parent_header: parent, parent_coinbase: coinbase, coinbase_merkle_branch: vec![21.into(), 22.into()] }
    }

    fn aux_bytes(h: &Header) -> Vec<u8> {
        borsh::to_vec(h.aux_pow.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn sedra_rpc_raw_header_carries_aux_pow() {
        let header = sample_header().with_aux_pow(sample_aux());
        let raw = RpcRawHeader::from(&header);
        assert_eq!(raw.aux_pow, borsh::to_vec(&sample_aux()).unwrap().to_rpc_hex());
        assert_eq!(RpcRawHeader::from(header.clone()).aux_pow, raw.aux_pow);
        let back = Header::try_from(&raw).unwrap();
        assert_eq!(back.hash, header.hash);
        assert_eq!(aux_bytes(&back), aux_bytes(&header));
        let back = Header::try_from(raw).unwrap();
        assert_eq!(aux_bytes(&back), aux_bytes(&header));

        let native = RpcRawHeader::from(&sample_header());
        assert!(native.aux_pow.is_empty());
        assert!(Header::try_from(native).unwrap().aux_pow.is_none());
    }

    #[test]
    fn sedra_rpc_bad_aux_pow_is_an_error() {
        let mut nested = sample_aux();
        nested.parent_header = nested.parent_header.clone().with_aux_pow(sample_aux());
        for (name, hex) in [
            ("not hex", "zz".to_string()),
            ("not a proof", "00ff".to_string()),
            ("nested", borsh::to_vec(&nested).unwrap().to_rpc_hex()),
            ("too large", "00".repeat(MAX_AUX_POW_SERIALIZED_LEN + 1)),
        ] {
            let mut raw = RpcRawHeader::from(&sample_header());
            raw.aux_pow = hex;
            assert!(Header::try_from(&raw).is_err(), "{name}");
        }
    }

    #[test]
    fn sedra_rpc_raw_header_json_without_aux_pow() {
        // pools that do not know the field keep working
        let raw = RpcRawHeader::from(&sample_header());
        let mut v = serde_json::to_value(&raw).unwrap();
        assert!(v.as_object_mut().unwrap().remove("auxPow").is_some());
        let back: RpcRawHeader = serde_json::from_value(v).unwrap();
        assert!(back.aux_pow.is_empty());
        assert_eq!(Header::try_from(back).unwrap().hash, sample_header().hash);
    }

    #[test]
    fn sedra_rpc_raw_header_wrpc_format() {
        let raw = RpcRawHeader::from(&sample_header().with_aux_pow(sample_aux()));
        let mut bytes = vec![];
        Serializer::serialize(&raw, &mut bytes).unwrap();
        assert_eq!(&bytes[..2], &2u16.to_le_bytes(), "format version 2");
        let back = <RpcRawHeader as Deserializer>::deserialize(&mut bytes.as_slice()).unwrap();
        assert_eq!(back.aux_pow, raw.aux_pow);

        // a format-1 message (older client) has no proof at the end and still decodes
        let native = RpcRawHeader::from(&sample_header());
        let mut v1 = vec![];
        Serializer::serialize(&native, &mut v1).unwrap();
        v1[..2].copy_from_slice(&1u16.to_le_bytes());
        v1.truncate(v1.len() - 4); // the empty string's length prefix
        let back = <RpcRawHeader as Deserializer>::deserialize(&mut v1.as_slice()).unwrap();
        assert!(back.aux_pow.is_empty());
        assert_eq!(Header::try_from(back).unwrap().hash, sample_header().hash);
    }
}
