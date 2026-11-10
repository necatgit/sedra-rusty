use crate::pb as protowire;
use kaspa_consensus_core::{
    auxpow::{AuxPow, MAX_AUX_POW_SERIALIZED_LEN},
    header::Header,
    BlueWorkType,
};
use log::debug;
use kaspa_hashes::Hash;

use super::error::ConversionError;
use super::option::TryIntoOptionEx;

// ----------------------------------------------------------------------------
// consensus_core to protowire
// ----------------------------------------------------------------------------

impl From<&Header> for protowire::BlockHeader {
    fn from(item: &Header) -> Self {
        Self {
            version: item.version.into(),
            parents: item.parents_by_level.iter().map(protowire::BlockLevelParents::from).collect(),
            hash_merkle_root: Some(item.hash_merkle_root.into()),
            accepted_id_merkle_root: Some(item.accepted_id_merkle_root.into()),
            utxo_commitment: Some(item.utxo_commitment.into()),
            timestamp: item.timestamp.try_into().expect("timestamp is always convertible to i64"),
            bits: item.bits,
            nonce: item.nonce,
            daa_score: item.daa_score,
            // We follow the golang specification of variable big-endian here
            blue_work: item.blue_work.to_be_bytes_var(),
            blue_score: item.blue_score,
            pruning_point: Some(item.pruning_point.into()),
            // SEDRA (M10): the merged-mining proof, borsh-encoded; empty for native blocks
            aux_pow: item.aux_pow.as_deref().map(|a| borsh::to_vec(a).expect("AuxPow is always borsh-serializable")).unwrap_or_default(),
        }
    }
}

impl From<&Vec<Hash>> for protowire::BlockLevelParents {
    fn from(item: &Vec<Hash>) -> Self {
        Self { parent_hashes: item.iter().map(|h| h.into()).collect() }
    }
}

// ----------------------------------------------------------------------------
// protowire to consensus_core
// ----------------------------------------------------------------------------

impl TryFrom<protowire::BlockHeader> for Header {
    type Error = ConversionError;
    fn try_from(item: protowire::BlockHeader) -> Result<Self, Self::Error> {
        let header = Self::new_finalized(
            item.version.try_into()?,
            item.parents.into_iter().map(Vec::<Hash>::try_from).collect::<Result<Vec<Vec<Hash>>, ConversionError>>()?,
            item.hash_merkle_root.try_into_ex()?,
            item.accepted_id_merkle_root.try_into_ex()?,
            item.utxo_commitment.try_into_ex()?,
            item.timestamp.try_into()?,
            item.bits,
            item.nonce,
            item.daa_score,
            // We follow the golang specification of variable big-endian here
            BlueWorkType::from_be_bytes_var(&item.blue_work)?,
            item.blue_score,
            item.pruning_point.try_into_ex()?,
        );
        Ok(with_p2p_aux_pow(header, &item.aux_pow))
    }
}

/// SEDRA (M10): reattaches the merged-mining proof received from a peer. An unusable proof (too large, undecodable,
/// structurally invalid) is dropped, never the header: the proof is outside the block hash, so any relay can replace
/// it, and it must not be able to make a natively valid block, or a whole batch of headers during sync, fail to
/// decode. A block that really needed the proof then fails its proof-of-work check, which is not cached as invalid,
/// so the block can still arrive later with a good proof. (ZKas learned this the hard way.)
fn with_p2p_aux_pow(header: Header, bytes: &[u8]) -> Header {
    if bytes.is_empty() {
        return header;
    }
    if bytes.len() > MAX_AUX_POW_SERIALIZED_LEN {
        debug!("dropping oversized aux proof ({} bytes) on header {}", bytes.len(), header.hash);
        return header;
    }
    match borsh::from_slice::<AuxPow>(bytes) {
        Ok(aux) => match aux.validate_structure() {
            Ok(()) => header.with_aux_pow(aux),
            Err(reason) => {
                debug!("dropping unusable aux proof on header {}: {}", header.hash, reason);
                header
            }
        },
        Err(e) => {
            debug!("dropping undecodable aux proof on header {}: {}", header.hash, e);
            header
        }
    }
}

impl TryFrom<protowire::BlockLevelParents> for Vec<Hash> {
    type Error = ConversionError;
    fn try_from(item: protowire::BlockLevelParents) -> Result<Self, Self::Error> {
        item.parent_hashes.into_iter().map(|x| x.try_into()).collect()
    }
}

#[cfg(test)]
mod sedra_aux_tests {
    use super::*;
    use kaspa_consensus_core::{auxpow::MAX_COINBASE_MERKLE_BRANCH, kaspa_parent::KaspaParentCoinbase, subnets::SUBNETWORK_ID_COINBASE};

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

    #[test]
    fn sedra_p2p_aux_pow_roundtrip() {
        let header = sample_header().with_aux_pow(sample_aux());
        let pb = protowire::BlockHeader::from(&header);
        assert_eq!(pb.aux_pow, borsh::to_vec(&sample_aux()).unwrap());
        let back = Header::try_from(pb).unwrap();
        assert_eq!(back.hash, header.hash);
        assert_eq!(borsh::to_vec(back.aux_pow.as_deref().unwrap()).unwrap(), borsh::to_vec(&sample_aux()).unwrap());

        let native = sample_header();
        let pb = protowire::BlockHeader::from(&native);
        assert!(pb.aux_pow.is_empty(), "a native header sends no proof bytes");
        assert!(Header::try_from(pb).unwrap().aux_pow.is_none());
    }

    #[test]
    fn sedra_p2p_bad_aux_pow_drops_the_proof_not_the_header() {
        let header = sample_header();
        let mut nested = sample_aux();
        nested.parent_header = nested.parent_header.clone().with_aux_pow(sample_aux());
        let mut long = sample_aux();
        long.coinbase_merkle_branch = vec![1.into(); MAX_COINBASE_MERKLE_BRANCH + 1];
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("garbage", vec![0xff; 100]),
            ("truncated", borsh::to_vec(&sample_aux()).unwrap()[..50].to_vec()),
            ("oversized", vec![0u8; MAX_AUX_POW_SERIALIZED_LEN + 1]),
            ("nested", borsh::to_vec(&nested).unwrap()),
            ("branch too long", borsh::to_vec(&long).unwrap()),
        ];
        for (name, bytes) in cases {
            let mut pb = protowire::BlockHeader::from(&header);
            pb.aux_pow = bytes;
            let back = Header::try_from(pb).unwrap_or_else(|e| panic!("{name}: the header must still decode: {e}"));
            assert_eq!(back.hash, header.hash, "{name}");
            assert!(back.aux_pow.is_none(), "{name}: the unusable proof is dropped");
        }
    }
}
