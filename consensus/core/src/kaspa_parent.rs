//! SEDRA (M10, SIP-0001 §4.1 and §4.6): the parts of a **Kaspa** block that a Sedra AuxPoW proof carries, hashed
//! exactly as Kaspa hashes them today.
//!
//! This node is based on rusty-kaspa v1.0.1. Kaspa has changed since:
//! - **Headers** are block version 2 with compressed parents, but the header hash still runs over the *expanded*
//!   parents with the same layout. A Kaspa parent header therefore travels as a normal [`crate::header::Header`]
//!   with expanded parents and is hashed by [`crate::hashing::header::hash`] (checked against real Kaspa headers in
//!   `consensus/src/processes/sedra_auxpow_parent_test.rs`).
//! - **Transactions**: since Kaspa's Toccata fork the coinbase is a **version 1** transaction, which this code base
//!   cannot represent or hash (v1 outputs carry a covenant field, and v1 always commits a mass field). So the parent
//!   coinbase has its own type here, [`KaspaParentCoinbase`], with Kaspa's current hashing (`hashing::tx::hash` in
//!   rusty-kaspa after Toccata) for exactly what a coinbase can contain: no inputs.
//!
//! A future Kaspa transaction version is not hashed ([`KaspaParentCoinbase::hash`] returns `None`): merged mining then
//! pauses until this node learns the new format, and native Sedra mining continues (SIP-0001 §4.6, decided Q20).

use crate::{hashing::HasherExtensions, subnets::SubnetworkId, tx::ScriptPublicKey};
use borsh::{BorshDeserialize, BorshSerialize};
use kaspa_hashes::{Hash, HasherBase};
use serde::{Deserialize, Serialize};

/// The newest Kaspa transaction version this node can hash (1 = Kaspa's Toccata transactions).
pub const MAX_KASPA_TX_VERSION: u16 = 1;

/// A Kaspa output's covenant binding (Toccata, transaction version 1 only).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct KaspaCovenant {
    pub authorizing_input: u16,
    pub covenant_id: Hash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct KaspaParentOutput {
    pub value: u64,
    pub script_public_key: ScriptPublicKey,
    /// Only version 1 transactions have this field; a version 0 coinbase with a covenant is malformed.
    pub covenant: Option<KaspaCovenant>,
}

/// The coinbase of a Kaspa parent block: leaf 0 of its transaction Merkle tree. A coinbase has no inputs, so none
/// are carried. Kaspa's consensus requires a coinbase's mass to be 0, so no mass is carried either: version 1 always
/// commits the mass, and it is committed as 0 here, which is what every real Kaspa coinbase does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct KaspaParentCoinbase {
    pub version: u16,
    pub outputs: Vec<KaspaParentOutput>,
    pub lock_time: u64,
    pub subnetwork_id: SubnetworkId,
    pub gas: u64,
    pub payload: Vec<u8>,
}

impl KaspaParentCoinbase {
    /// Kaspa's transaction hash of this coinbase (the Merkle leaf), or `None` if this node cannot hash it: an
    /// unknown (future) transaction version, or a version 0 output with a covenant.
    pub fn hash(&self) -> Option<Hash> {
        if self.version > MAX_KASPA_TX_VERSION || (self.version == 0 && self.outputs.iter().any(|o| o.covenant.is_some())) {
            return None;
        }
        let mut hasher = kaspa_hashes::TransactionHash::new();
        hasher.update(self.version.to_le_bytes()).write_len(0); // no inputs
        hasher.write_len(self.outputs.len());
        for output in &self.outputs {
            hasher
                .update(output.value.to_le_bytes())
                .update(output.script_public_key.version().to_le_bytes())
                .write_var_bytes(output.script_public_key.script());
            if self.version >= 1 {
                hasher.write_bool(output.covenant.is_some());
                if let Some(covenant) = &output.covenant {
                    hasher.write_u16(covenant.authorizing_input);
                    hasher.update(covenant.covenant_id);
                }
            }
        }
        hasher.update(self.lock_time.to_le_bytes()).update(self.subnetwork_id.clone()).update(self.gas.to_le_bytes());
        hasher.write_var_bytes(&self.payload);
        // Mass commitment: version 0 commits it only when > 0, version 1 always. A coinbase's mass is 0.
        if self.version >= 1 {
            hasher.write_u64(0);
        }
        Some(hasher.finalize())
    }
}

/// The Merkle root reached from leaf 0 (the coinbase) through `branch`, the right siblings from the leaf upwards.
/// Leaf 0 is always the left child, so a wrong or reordered branch cannot reach the root. An empty branch means the
/// block holds only the coinbase, which is then the root itself (Kaspa's `calc_merkle_root` with one leaf).
pub fn merkle_root_from_coinbase(coinbase_hash: Hash, branch: &[Hash]) -> Hash {
    branch.iter().fold(coinbase_hash, |acc, sibling| kaspa_merkle::merkle_hash(acc, *sibling))
}
