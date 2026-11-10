//! SEDRA (M10, SIP-0001 §4.1-§4.5): the merged-mining proof (AuxPoW) a Sedra block can carry instead of its own proof
//! of work, and its **structural** checks. The **work** check (the Kaspa parent's kHeavyHash against the Sedra block's
//! target) and the activation point are separate steps.
//!
//! The binding chain, from the work back to the Sedra block:
//! `kHeavyHash(parent header)` → `parent.hash_merkle_root` → Merkle branch → Kaspa coinbase (leaf 0) → `SDM1`
//! commitment in its payload → `H("sedra_aux_bind1" || genesis || nonce-free Sedra header hash)`.
//! In addition the Sedra header's nonce must equal the first 8 bytes of the parent's hash (§4.3), so one Sedra block
//! has exactly one valid proof.
//!
//! The proof travels next to the Sedra header, outside its hash. Nothing here trusts a cached hash: the parent's hash
//! is always recomputed from its fields. Follows the ZKas design (zkas-rusty `consensus/core/src/auxpow.rs`), with
//! Sedra's own tag and domain string, a base64url commitment (Q25) and Kaspa's current coinbase format
//! ([`crate::kaspa_parent`]). The parent is a [`Header`] and so has a witness field too; a proof whose parent carries
//! a proof is rejected ([`AuxPowError::NestedProof`]), so proofs cannot nest (nesting depth 1, §4.1).

use crate::{
    hashing,
    header::Header,
    kaspa_parent::{merkle_root_from_coinbase, KaspaParentCoinbase},
};
use borsh::{BorshDeserialize, BorshSerialize};
use kaspa_hashes::{Hash, HasherBase};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The tag in the Kaspa coinbase payload that precedes the Sedra commitment (SIP-0001 §4.2). It contains characters
/// outside `0-9a-f`, so it cannot occur inside another chain's hex commitment (such as ZKas's `ZKM1…`).
pub const MERGE_MINE_TAG: [u8; 4] = *b"SDM1";

/// Domain string of the commitment hash; binds the commitment to Sedra merged mining (§4.2).
pub const COMMITMENT_DOMAIN: &[u8] = b"sedra_aux_bind1";

/// Length of the base64url (no padding) encoding of the 32-byte commitment.
pub const COMMITMENT_B64_LEN: usize = 43;

/// The whole tagged commitment as it appears in the payload: 4 + 43 = 47 bytes.
pub const TAGGED_COMMITMENT_LEN: usize = MERGE_MINE_TAG.len() + COMMITMENT_B64_LEN;

/// Longest accepted Merkle branch: one entry per level of the parent's transaction tree, so 64 covers any block that
/// can exist. Each entry costs a hash, so an unbounded branch would be free work for an attacker (ZKas value).
pub const MAX_COINBASE_MERKLE_BRANCH: usize = 64;

/// Largest accepted serialized proof. A real Kaspa header, coinbase and branch are a few kilobytes; nothing else
/// bounds the parent's parents or the coinbase's outputs (ZKas value, §4.1).
pub const MAX_AUX_POW_SERIALIZED_LEN: usize = 256 * 1024;

/// A merged-mining proof: the Kaspa parent block's header, its coinbase and the coinbase's Merkle branch.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxPow {
    /// The Kaspa parent header with expanded parents. Its cached `hash` field is never used.
    pub parent_header: Header,
    /// Leaf 0 of the parent's transaction tree.
    pub parent_coinbase: KaspaParentCoinbase,
    /// Right siblings from the coinbase up to `parent_header.hash_merkle_root`; empty if the coinbase is the only
    /// transaction.
    pub coinbase_merkle_branch: Vec<Hash>,
}

/// Why a proof does not bind the Sedra block. Every check of [`AuxPow::verify_binding`] has its own variant, so tests
/// and logs can tell which rule failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuxPowError {
    BranchTooLong(usize),
    TooLarge(usize),
    /// The parent header carries a proof itself. Kaspa has no such field, so this is never legitimate.
    NestedProof,
    UnhashableCoinbase(u16),
    /// The coinbase payload has no well-formed `SDM1` commitment, or more than one `SDM1` tag.
    NoUniqueCommitment,
    WrongCommitment,
    NotInParentMerkleRoot,
    /// The Sedra header's nonce is not the first 8 bytes of the parent's hash.
    NonceNotBound { expected: u64, actual: u64 },
}

impl fmt::Display for AuxPowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BranchTooLong(n) => write!(f, "coinbase merkle branch too long: {n} > {MAX_COINBASE_MERKLE_BRANCH}"),
            Self::TooLarge(n) => write!(f, "aux proof too large: {n} > {MAX_AUX_POW_SERIALIZED_LEN} bytes"),
            Self::NestedProof => write!(f, "the parent header carries an aux proof itself"),
            Self::UnhashableCoinbase(v) => write!(f, "parent coinbase cannot be hashed (transaction version {v})"),
            Self::NoUniqueCommitment => write!(f, "parent coinbase has no unique well-formed SDM1 commitment"),
            Self::WrongCommitment => write!(f, "parent coinbase commits to a different block"),
            Self::NotInParentMerkleRoot => write!(f, "parent coinbase is not under the parent's hash_merkle_root"),
            Self::NonceNotBound { expected, actual } => write!(f, "nonce {actual} is not bound to the parent (expected {expected})"),
        }
    }
}

impl std::error::Error for AuxPowError {}

impl AuxPow {
    /// The value a parent commits to (§4.2): `H("sedra_aux_bind1" || genesis || nonce-free header hash)` with the
    /// block-hash hasher. The genesis keeps a testnet or devnet commitment from being reused on mainnet.
    pub fn commitment(genesis: Hash, nonce_free_header_hash: Hash) -> Hash {
        let mut hasher = kaspa_hashes::BlockHash::new();
        hasher.update(COMMITMENT_DOMAIN).update(genesis).update(nonce_free_header_hash);
        hasher.finalize()
    }

    /// The Sedra header hash with the nonce set to 0 (§4.2). The nonce is left out because it is then set to the
    /// parent's [`Self::nonce_binding`]; every other field, the timestamp included, stays bound.
    pub fn nonce_free_header_hash(header: &Header) -> Hash {
        hashing::header::hash_override_nonce_time(header, 0, header.timestamp)
    }

    /// The nonce a merged-mined Sedra block must carry (§4.3): the first 8 bytes (little endian) of the parent's
    /// hash, recomputed from the parent's fields.
    pub fn nonce_binding(parent_header: &Header) -> u64 {
        let h = hashing::header::hash(parent_header).as_bytes();
        u64::from_le_bytes(h[..8].try_into().expect("a hash has 32 bytes"))
    }

    /// The `SDM1` commitment as a pool writes it into the Kaspa coinbase payload (`extraData`): 47 ASCII bytes.
    pub fn tagged_commitment(commitment: Hash) -> [u8; TAGGED_COMMITMENT_LEN] {
        let mut out = [0u8; TAGGED_COMMITMENT_LEN];
        out[..4].copy_from_slice(&MERGE_MINE_TAG);
        out[4..].copy_from_slice(&base64url_encode_32(&commitment.as_bytes()));
        out
    }

    /// The commitment in `payload`, if `SDM1` occurs **exactly once** and is followed by a canonical base64url
    /// encoding of 32 bytes. A second tag anywhere makes it ambiguous, so nothing is returned (§4.2).
    pub fn payload_commitment(payload: &[u8]) -> Option<Hash> {
        let mut found = None;
        let mut i = 0;
        while i + MERGE_MINE_TAG.len() <= payload.len() {
            if payload[i..i + MERGE_MINE_TAG.len()] != MERGE_MINE_TAG {
                i += 1;
                continue;
            }
            if found.is_some() {
                return None; // a second tag: ambiguous
            }
            let start = i + MERGE_MINE_TAG.len();
            let encoded = payload.get(start..start + COMMITMENT_B64_LEN)?; // truncated: malformed
            found = Some(Hash::from_bytes(base64url_decode_32(encoded)?));
            i = start; // the tag cannot overlap itself, so continue after it
        }
        found
    }

    /// Whether `payload` carries a well-formed Sedra commitment, i.e. whether a block with this coinbase could be a
    /// Sedra parent. A Sedra block's own coinbase must not (self-parent ban, §4.5); other chains' tags are allowed.
    pub fn payload_carries_commitment(payload: &[u8]) -> bool {
        Self::payload_commitment(payload).is_some()
    }

    /// Heap and inline bytes of this proof, for cache budgets. Every variable-length part counts (the coinbase's
    /// output scripts and payload too), so a proof cannot hold more memory than the cache believes.
    pub fn estimate_mem_bytes(&self) -> usize {
        size_of::<Self>()
            + self.parent_header.parents_by_level.iter().map(|l| l.len() * size_of::<Hash>() + size_of::<Vec<Hash>>()).sum::<usize>()
            + self.parent_header.aux_pow.as_ref().map_or(0, |a| a.estimate_mem_bytes())
            + self.parent_coinbase.outputs.iter().map(|o| size_of_val(o) + o.script_public_key.script().len()).sum::<usize>()
            + self.parent_coinbase.payload.len()
            + self.coinbase_merkle_branch.len() * size_of::<Hash>()
    }

    /// Size limits every decode edge applies before a proof is stored, relayed or hashed (§4.1).
    pub fn validate_structure(&self) -> Result<(), AuxPowError> {
        if self.coinbase_merkle_branch.len() > MAX_COINBASE_MERKLE_BRANCH {
            return Err(AuxPowError::BranchTooLong(self.coinbase_merkle_branch.len()));
        }
        if self.parent_header.aux_pow.is_some() {
            return Err(AuxPowError::NestedProof);
        }
        let len = borsh::to_vec(self).map(|v| v.len()).unwrap_or(usize::MAX);
        if len > MAX_AUX_POW_SERIALIZED_LEN {
            return Err(AuxPowError::TooLarge(len));
        }
        Ok(())
    }

    /// The structural half of SIP-0001 §4.4 for a Sedra `header` on the network with `genesis`: limits, a hashable
    /// coinbase, exactly one `SDM1` commitment to this header, the coinbase under the parent's Merkle root, and the
    /// nonce binding. The parent's proof of work is checked separately.
    pub fn verify_binding(&self, genesis: Hash, header: &Header) -> Result<(), AuxPowError> {
        self.validate_structure()?;
        let coinbase_hash = self.parent_coinbase.hash().ok_or(AuxPowError::UnhashableCoinbase(self.parent_coinbase.version))?;
        let committed = Self::payload_commitment(&self.parent_coinbase.payload).ok_or(AuxPowError::NoUniqueCommitment)?;
        if committed != Self::commitment(genesis, Self::nonce_free_header_hash(header)) {
            return Err(AuxPowError::WrongCommitment);
        }
        if merkle_root_from_coinbase(coinbase_hash, &self.coinbase_merkle_branch) != self.parent_header.hash_merkle_root {
            return Err(AuxPowError::NotInParentMerkleRoot);
        }
        let expected = Self::nonce_binding(&self.parent_header);
        if header.nonce != expected {
            return Err(AuxPowError::NonceNotBound { expected, actual: header.nonce });
        }
        Ok(())
    }
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding (RFC 4648 §5) of 32 bytes: 43 characters.
fn base64url_encode_32(bytes: &[u8; 32]) -> [u8; COMMITMENT_B64_LEN] {
    let mut out = [0u8; COMMITMENT_B64_LEN];
    let mut o = 0;
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..chunk.len() + 1 {
            out[o] = B64URL[(n >> (18 - 6 * k) & 63) as usize];
            o += 1;
        }
    }
    out
}

fn b64url_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a' + 26) as u32),
        b'0'..=b'9' => Some((c - b'0' + 52) as u32),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

/// Decodes exactly 43 base64url characters to 32 bytes. Only the canonical encoding is accepted: the last character
/// carries 2 unused bits that must be 0, so each hash has exactly one valid spelling.
fn base64url_decode_32(s: &[u8]) -> Option<[u8; 32]> {
    if s.len() != COMMITMENT_B64_LEN {
        return None;
    }
    let mut out = [0u8; 32];
    let mut o = 0;
    for group in s.chunks(4) {
        let mut n = 0u32;
        for (k, &c) in group.iter().enumerate() {
            n |= b64url_value(c)? << (18 - 6 * k);
        }
        let take = group.len() - 1; // 4 chars -> 3 bytes, the final 3 chars -> 2 bytes
        if take == 2 && n & 0xff != 0 {
            return None; // non-zero unused bits: not canonical
        }
        for k in 0..take {
            out[o] = (n >> (16 - 8 * k)) as u8;
            o += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // Expected values computed independently with Python (hashlib.blake2b keyed "BlockHash", base64.urlsafe_b64encode)
    const GENESIS: &str = "7c380ae690f9eaa64f300451d5e466371113c38b96ed96fe3f1e934869355ffa";
    const VECTORS: [(&str, &str, &str); 3] = [
        (
            "0000000000000000000000000000000000000000000000000000000000000000",
            "ae454a0070e3272a3f8beb53bae0fc99ca1ef8998399dafab34e25faaf05612b",
            "SDM1rkVKAHDjJyo_i-tTuuD8mcoe-JmDmdr6s04l-q8FYSs",
        ),
        (
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "c86e35c77203c38a7d9a5118b16956c76996720310c65fb0a4c8f6a2799650e8",
            "SDM1yG41x3IDw4p9mlEYsWlWx2mWcgMQxl-wpMj2onmWUOg",
        ),
        (
            "d2f08e8cb2d509a456bab6e2692319f8d35669c0a4396812400a68d081b6bb42",
            "bebb9cbfc442816ac310afd89a820d0ee4d864743381f02f24cf5fd190f3cc7f",
            "SDM1vrucv8RCgWrDEK_YmoINDuTYZHQzgfAvJM9f0ZDzzH8",
        ),
    ];

    fn h(s: &str) -> Hash {
        Hash::from_str(s).unwrap()
    }

    #[test]
    fn sedra_auxpow_commitment_matches_independent_vectors() {
        for (nfh, commitment, tagged) in VECTORS {
            let c = AuxPow::commitment(h(GENESIS), h(nfh));
            assert_eq!(c, h(commitment));
            assert_eq!(&AuxPow::tagged_commitment(c)[..], tagged.as_bytes());
            assert_eq!(AuxPow::payload_commitment(tagged.as_bytes()), Some(c));
        }
        // the genesis is bound: another network's commitment differs
        assert_ne!(AuxPow::commitment(h(VECTORS[0].0), h(VECTORS[0].0)), h(VECTORS[0].1));
    }

    #[test]
    fn sedra_auxpow_base64url_matches_rfc4648() {
        // Python: base64.urlsafe_b64encode(raw).rstrip(b"=")
        let raw: [u8; 32] = core::array::from_fn(|i| i as u8);
        assert_eq!(&base64url_encode_32(&raw), b"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8");
        let raw: [u8; 32] = core::array::from_fn(|i| if i % 2 == 0 { 0xfb } else { 0xff });
        assert_eq!(&base64url_encode_32(&raw), b"-__7__v_-__7__v_-__7__v_-__7__v_-__7__v_-_8");
        assert_eq!(base64url_decode_32(b"-__7__v_-__7__v_-__7__v_-__7__v_-__7__v_-_8"), Some(raw));
    }

    #[test]
    fn sedra_auxpow_payload_commitment_rules() {
        let tagged = VECTORS[2].2.as_bytes();
        let c = h(VECTORS[2].1);
        let wrap = |pre: &[u8], post: &[u8]| [pre, tagged, post].concat();

        // anywhere in the payload, next to other data such as a ZKas commitment
        let zkas = b"ZKM1bebb9cbfc442816ac310afd89a820d0ee4d864743381f02f24cf5fd190f3cc7f";
        assert_eq!(AuxPow::payload_commitment(&wrap(b"PuddlePool/", zkas)), Some(c));
        assert_eq!(AuxPow::payload_commitment(&wrap(zkas, b"")), Some(c));
        assert!(AuxPow::payload_carries_commitment(&wrap(b"x", b"y")));

        // exactly once
        assert_eq!(AuxPow::payload_commitment(&[tagged, tagged].concat()), None, "two commitments");
        assert_eq!(AuxPow::payload_commitment(&wrap(b"", b"SDM1")), None, "a second, truncated tag");
        assert_eq!(AuxPow::payload_commitment(zkas), None, "no tag");
        assert!(!AuxPow::payload_carries_commitment(zkas), "another chain's tag is not a Sedra commitment");

        // well-formed and canonical
        assert_eq!(AuxPow::payload_commitment(&tagged[..TAGGED_COMMITMENT_LEN - 1]), None, "truncated");
        let mut bad = tagged.to_vec();
        bad[10] = b'+'; // standard base64, not base64url
        assert_eq!(AuxPow::payload_commitment(&bad), None, "invalid character");
        let mut noncanonical = tagged.to_vec();
        noncanonical[TAGGED_COMMITMENT_LEN - 1] = b'9'; // '8' (60) -> '9' (61): sets an unused low bit
        assert_eq!(AuxPow::payload_commitment(&noncanonical), None, "non-canonical last character");
    }
}
