//! SEDRA (M10 step 3): the work half of SIP-0001 §4.4 (dual acceptance) and the block level of §4.7.
//!
//! The proof type and its structural checks live in [`kaspa_consensus_core::auxpow`]; this module adds what needs the
//! kHeavyHash [`State`]: does the Kaspa parent's work meet the **Sedra** target of this block. Both chains run the same
//! hash, so a Kaspa hash that misses Kaspa's hard target routinely meets Sedra's easier one. The parent's own `bits`,
//! parents, DAA score and acceptance by Kaspa are deliberately not checked (§4.4).
//!
//! Not wired into the header pipeline yet; the activation point and the proof's transport come in the next step.
//! Modeled on zkas-rusty `consensus/pow/src/auxpow.rs` (bound rule only: Sedra has no legacy commitment).

use kaspa_consensus_core::{
    auxpow::{AuxPow, AuxPowError},
    header::Header,
    BlockLevel,
};
use kaspa_hashes::Hash;
use kaspa_math::Uint256;

use crate::{calc_level_from_pow, State};

/// The Kaspa parent's kHeavyHash, computed from its fields and compared against the Sedra target, never the parent's.
pub fn parent_pow(aux: &AuxPow) -> Uint256 {
    State::new(&aux.parent_header).calculate_pow(aux.parent_header.nonce)
}

/// Why a proof does not make a Sedra block valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuxPowRejection {
    /// A structural rule of §4.1-§4.3 failed.
    Binding(AuxPowError),
    /// The parent's kHeavyHash is above the Sedra target.
    InsufficientWork,
}

/// Full proof check for the Sedra `header` on the network with `genesis` (§4.4 item 2). The cheap binding checks run
/// before the parent's kHeavyHash. Returns the parent's work value, which is the block's work for the block level.
pub fn verify_aux_pow(aux: &AuxPow, genesis: Hash, header: &Header) -> Result<Uint256, AuxPowRejection> {
    aux.verify_binding(genesis, header).map_err(AuxPowRejection::Binding)?;
    let pow = parent_pow(aux);
    if pow > Uint256::from_compact_target_bits(header.bits) {
        return Err(AuxPowRejection::InsufficientWork);
    }
    Ok(pow)
}

/// Whether the merged-mining path is open for a header: closed before activation, open after it on the network with
/// `genesis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxRule {
    Off,
    Active { genesis: Hash },
}

impl AuxRule {
    pub fn new(merged_mining_active: bool, genesis: Hash) -> Self {
        if merged_mining_active {
            AuxRule::Active { genesis }
        } else {
            AuxRule::Off
        }
    }
}

/// Outcome of the dual proof-of-work check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PowCheck {
    /// The header's own kHeavyHash meets its target.
    Native(Uint256),
    /// The native check failed and a valid proof was accepted; the value is the parent's work.
    Aux(Uint256),
    /// Neither path passed. `aux` says why the proof failed, `None` if there was none or the path was closed.
    Failed { native_pow: Uint256, aux: Option<AuxPowRejection> },
}

impl PowCheck {
    pub fn passed(&self) -> bool {
        !matches!(self, PowCheck::Failed { .. })
    }

    /// The work value for the block level: the parent's for an aux block, the header's own otherwise.
    pub fn pow(&self) -> Uint256 {
        match self {
            PowCheck::Native(pow) | PowCheck::Aux(pow) => *pow,
            PowCheck::Failed { native_pow, .. } => *native_pow,
        }
    }
}

/// The dual-acceptance gate (§4.4). The native path is tried first: the proof is not covered by the block hash and can
/// be stripped or replaced in transit, so it must never invalidate a block that is valid on its own. Before activation
/// the proof is ignored entirely.
pub fn check_pow_gated(header: &Header, aux: Option<&AuxPow>, rule: AuxRule) -> PowCheck {
    let (native_ok, native_pow) = State::new(header).check_pow(header.nonce);
    if native_ok {
        return PowCheck::Native(native_pow);
    }
    let aux = match (rule, aux) {
        (AuxRule::Active { genesis }, Some(a)) => match verify_aux_pow(a, genesis, header) {
            Ok(pow) => return PowCheck::Aux(pow),
            Err(e) => Some(e),
        },
        _ => None,
    };
    PowCheck::Failed { native_pow, aux }
}

/// Aux-aware [`crate::calc_block_level_check_pow`] (§4.7): an aux-accepted block's level comes from the parent's work.
/// Anything that re-derives a stored level (pruning-proof validation) must use this with the same rule, or it computes
/// a far lower level for merged-mined blocks than the one stored at acceptance.
pub fn calc_block_level_check_pow_gated(
    header: &Header,
    max_block_level: BlockLevel,
    aux: Option<&AuxPow>,
    rule: AuxRule,
) -> (BlockLevel, bool) {
    if header.parents_by_level.is_empty() {
        return (max_block_level, true); // Genesis has the max block level
    }
    let check = check_pow_gated(header, aux, rule);
    (calc_level_from_pow(check.pow(), max_block_level), check.passed())
}
