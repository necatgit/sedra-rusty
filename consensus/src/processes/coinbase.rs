use kaspa_consensus_core::{
    coinbase::*,
    config::params::{block_in_treasury_allocation_window, ForkedParam, TreasuryAllocationWindow},
    errors::coinbase::{CoinbaseError, CoinbaseResult},
    subnets,
    tx::{ScriptPublicKey, ScriptVec, Transaction, TransactionOutput},
    BlockHashMap, BlockHashSet,
};
use std::convert::TryInto;

use crate::{constants, model::stores::ghostdag::GhostdagData};

const LENGTH_OF_BLUE_SCORE: usize = size_of::<u64>();
const LENGTH_OF_SUBSIDY: usize = size_of::<u64>();
const LENGTH_OF_SCRIPT_PUB_KEY_VERSION: usize = size_of::<u16>();
const LENGTH_OF_SCRIPT_PUB_KEY_LENGTH: usize = size_of::<u8>();

const MIN_PAYLOAD_LENGTH: usize =
    LENGTH_OF_BLUE_SCORE + LENGTH_OF_SUBSIDY + LENGTH_OF_SCRIPT_PUB_KEY_VERSION + LENGTH_OF_SCRIPT_PUB_KEY_LENGTH;

// We define a year as 365.25 days and a month as 365.25 / 12 = 30.4375
// SECONDS_PER_MONTH = 30.4375 * 24 * 60 * 60
const SECONDS_PER_MONTH: u64 = 2629800;

pub const SUBSIDY_BY_MONTH_TABLE_SIZE: usize = 426;
pub type SubsidyByMonthTable = [u64; SUBSIDY_BY_MONTH_TABLE_SIZE];

#[derive(Clone)]
pub struct CoinbaseManager {
    coinbase_payload_script_public_key_max_len: u8,
    max_coinbase_payload_len: usize,
    deflationary_phase_daa_score: u64,
    pre_deflationary_phase_base_subsidy: u64,
    bps: ForkedParam<u64>,

    /// Precomputed subsidy by month tables (for before and after the Crescendo hardfork)
    subsidy_by_month_table_before: SubsidyByMonthTable,
    subsidy_by_month_table_after: SubsidyByMonthTable,

    /// The crescendo activation DAA score where BPS increased from 1 to 10.
    /// This score is required here long-term (and not only for the actual forking), in
    /// order to correctly determine the subsidy month from the live DAA score of the network   
    crescendo_activation_daa_score: u64,

    /// SEDRA v2.0.0 treasury allocation (dormant when the windows or the script are empty)
    treasury_allocation_windows: &'static [TreasuryAllocationWindow],
    treasury_script_public_key: Option<ScriptPublicKey>,
}

/// Struct used to streamline payload parsing
struct PayloadParser<'a> {
    remaining: &'a [u8], // The unparsed remainder
}

impl<'a> PayloadParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { remaining: data }
    }

    /// Returns a slice with the first `n` bytes of `remaining`, while setting `remaining` to the remaining part
    fn take(&mut self, n: usize) -> &[u8] {
        let (segment, remaining) = self.remaining.split_at(n);
        self.remaining = remaining;
        segment
    }
}

impl CoinbaseManager {
    pub fn new(
        coinbase_payload_script_public_key_max_len: u8,
        max_coinbase_payload_len: usize,
        deflationary_phase_daa_score: u64,
        pre_deflationary_phase_base_subsidy: u64,
        bps: ForkedParam<u64>,
    ) -> Self {
        // Precomputed subsidy by month table for the actual block per second rate
        // Here values are rounded up so that we keep the same number of rewarding months as in the original 1 BPS table.
        // In a 10 BPS network, the induced increase in total rewards is 51 KAS (see tests::calc_high_bps_total_rewards_delta())
        let subsidy_by_month_table_before: SubsidyByMonthTable =
            core::array::from_fn(|i| SUBSIDY_BY_MONTH_TABLE[i].div_ceil(bps.before()));
        let subsidy_by_month_table_after: SubsidyByMonthTable =
            core::array::from_fn(|i| SUBSIDY_BY_MONTH_TABLE[i].div_ceil(bps.after()));
        Self {
            coinbase_payload_script_public_key_max_len,
            max_coinbase_payload_len,
            deflationary_phase_daa_score,
            pre_deflationary_phase_base_subsidy,
            bps,
            subsidy_by_month_table_before,
            subsidy_by_month_table_after,
            crescendo_activation_daa_score: bps.activation().daa_score(),
            treasury_allocation_windows: &[],
            treasury_script_public_key: None,
        }
    }

    /// SEDRA v2.0.0: enables the treasury allocation output (sedrad's `treasuryAllocationOutput`).
    pub fn with_treasury_allocation(mut self, windows: &'static [TreasuryAllocationWindow], script_public_key: &'static [u8]) -> Self {
        self.treasury_allocation_windows = windows;
        self.treasury_script_public_key =
            if script_public_key.is_empty() { None } else { Some(ScriptPublicKey::new(0, ScriptVec::from_slice(script_public_key))) };
        self
    }

    /// SEDRA v2.0.0: sedrad's `treasuryAllocationOutput` for a block with the given DAA score. The first matching window
    /// with a non-zero amount pays. Dormant when no window or no script is configured.
    pub fn treasury_allocation_output(&self, daa_score: u64) -> Option<TransactionOutput> {
        let spk = self.treasury_script_public_key.as_ref()?;
        if self.treasury_allocation_windows.is_empty() {
            return None;
        }
        self.treasury_allocation_windows
            .iter()
            .filter(|w| w.per_block_seep != 0)
            .find(|w| block_in_treasury_allocation_window(daa_score, w.activation_daa_score, w.window_blocks))
            .map(|w| TransactionOutput::new(w.per_block_seep, spk.clone()))
    }

    #[cfg(test)]
    #[inline]
    pub fn bps(&self) -> ForkedParam<u64> {
        self.bps
    }

    pub fn expected_coinbase_transaction<T: AsRef<[u8]>>(
        &self,
        daa_score: u64,
        miner_data: MinerData<T>,
        ghostdag_data: &GhostdagData,
        mergeset_rewards: &BlockHashMap<BlockRewardData>,
        mergeset_non_daa: &BlockHashSet,
    ) -> CoinbaseResult<CoinbaseTransactionTemplate> {
        let mut outputs = Vec::with_capacity(ghostdag_data.mergeset_blues.len() + 1); // + 1 for possible red reward

        // Add an output for each mergeset blue block (∩ DAA window), paying to the script reported by the block.
        // Note that combinatorically it is nearly impossible for a blue block to be non-DAA
        for blue in ghostdag_data.mergeset_blues.iter().filter(|h| !mergeset_non_daa.contains(h)) {
            let reward_data = mergeset_rewards.get(blue).unwrap();
            if reward_data.subsidy + reward_data.total_fees > 0 {
                outputs
                    .push(TransactionOutput::new(reward_data.subsidy + reward_data.total_fees, reward_data.script_public_key.clone()));
            }
        }

        // Collect all rewards from mergeset reds ∩ DAA window and create a
        // single output rewarding all to the current block (the "merging" block)
        let mut red_reward = 0u64;

        // bps activation = crescendo activation
        if self.bps.activation().is_active(daa_score) {
            for red in ghostdag_data.mergeset_reds.iter() {
                let reward_data = mergeset_rewards.get(red).unwrap();
                if mergeset_non_daa.contains(red) {
                    red_reward += reward_data.total_fees;
                } else {
                    red_reward += reward_data.subsidy + reward_data.total_fees;
                }
            }
        } else {
            for red in ghostdag_data.mergeset_reds.iter().filter(|h| !mergeset_non_daa.contains(h)) {
                let reward_data = mergeset_rewards.get(red).unwrap();
                red_reward += reward_data.subsidy + reward_data.total_fees;
            }
        }

        if red_reward > 0 {
            outputs.push(TransactionOutput::new(red_reward, miner_data.script_public_key.clone()));
        }

        // SEDRA v2.0.0: the treasury allocation output comes after the blue and red reward outputs, as in sedrad
        if let Some(treasury_output) = self.treasury_allocation_output(daa_score) {
            outputs.push(treasury_output);
        }

        // Build the current block's payload
        let subsidy = self.calc_block_subsidy(daa_score);
        let payload = self.serialize_coinbase_payload(&CoinbaseData { blue_score: ghostdag_data.blue_score, subsidy, miner_data })?;

        Ok(CoinbaseTransactionTemplate {
            tx: Transaction::new(constants::TX_VERSION, vec![], outputs, 0, subnets::SUBNETWORK_ID_COINBASE, 0, payload),
            has_red_reward: red_reward > 0,
        })
    }

    pub fn serialize_coinbase_payload<T: AsRef<[u8]>>(&self, data: &CoinbaseData<T>) -> CoinbaseResult<Vec<u8>> {
        let script_pub_key_len = data.miner_data.script_public_key.script().len();
        if script_pub_key_len > self.coinbase_payload_script_public_key_max_len as usize {
            return Err(CoinbaseError::PayloadScriptPublicKeyLenAboveMax(
                script_pub_key_len,
                self.coinbase_payload_script_public_key_max_len,
            ));
        }
        let payload: Vec<u8> = data.blue_score.to_le_bytes().iter().copied()                    // Blue score                   (u64)
            .chain(data.subsidy.to_le_bytes().iter().copied())                                  // Subsidy                      (u64)
            .chain(data.miner_data.script_public_key.version().to_le_bytes().iter().copied())   // Script public key version    (u16)
            .chain((script_pub_key_len as u8).to_le_bytes().iter().copied())                    // Script public key length     (u8)
            .chain(data.miner_data.script_public_key.script().iter().copied())                  // Script public key            
            .chain(data.miner_data.extra_data.as_ref().iter().copied())                         // Extra data
            .collect();

        Ok(payload)
    }

    pub fn modify_coinbase_payload<T: AsRef<[u8]>>(&self, mut payload: Vec<u8>, miner_data: &MinerData<T>) -> CoinbaseResult<Vec<u8>> {
        let script_pub_key_len = miner_data.script_public_key.script().len();
        if script_pub_key_len > self.coinbase_payload_script_public_key_max_len as usize {
            return Err(CoinbaseError::PayloadScriptPublicKeyLenAboveMax(
                script_pub_key_len,
                self.coinbase_payload_script_public_key_max_len,
            ));
        }

        // Keep only blue score and subsidy. Note that truncate does not modify capacity, so
        // the usual case where the payloads are the same size will not trigger a reallocation
        payload.truncate(LENGTH_OF_BLUE_SCORE + LENGTH_OF_SUBSIDY);
        payload.extend(
            miner_data.script_public_key.version().to_le_bytes().iter().copied() // Script public key version (u16)
                .chain((script_pub_key_len as u8).to_le_bytes().iter().copied()) // Script public key length  (u8)
                .chain(miner_data.script_public_key.script().iter().copied())    // Script public key
                .chain(miner_data.extra_data.as_ref().iter().copied()), // Extra data
        );

        Ok(payload)
    }

    pub fn deserialize_coinbase_payload<'a>(&self, payload: &'a [u8]) -> CoinbaseResult<CoinbaseData<&'a [u8]>> {
        if payload.len() < MIN_PAYLOAD_LENGTH {
            return Err(CoinbaseError::PayloadLenBelowMin(payload.len(), MIN_PAYLOAD_LENGTH));
        }

        if payload.len() > self.max_coinbase_payload_len {
            return Err(CoinbaseError::PayloadLenAboveMax(payload.len(), self.max_coinbase_payload_len));
        }

        let mut parser = PayloadParser::new(payload);

        let blue_score = u64::from_le_bytes(parser.take(LENGTH_OF_BLUE_SCORE).try_into().unwrap());
        let subsidy = u64::from_le_bytes(parser.take(LENGTH_OF_SUBSIDY).try_into().unwrap());
        let script_pub_key_version = u16::from_le_bytes(parser.take(LENGTH_OF_SCRIPT_PUB_KEY_VERSION).try_into().unwrap());
        let script_pub_key_len = u8::from_le_bytes(parser.take(LENGTH_OF_SCRIPT_PUB_KEY_LENGTH).try_into().unwrap());

        if script_pub_key_len > self.coinbase_payload_script_public_key_max_len {
            return Err(CoinbaseError::PayloadScriptPublicKeyLenAboveMax(
                script_pub_key_len as usize,
                self.coinbase_payload_script_public_key_max_len,
            ));
        }

        if parser.remaining.len() < script_pub_key_len as usize {
            return Err(CoinbaseError::PayloadCantContainScriptPublicKey(
                payload.len(),
                MIN_PAYLOAD_LENGTH + script_pub_key_len as usize,
            ));
        }

        let script_public_key =
            ScriptPublicKey::new(script_pub_key_version, ScriptVec::from_slice(parser.take(script_pub_key_len as usize)));
        let extra_data = parser.remaining;

        Ok(CoinbaseData { blue_score, subsidy, miner_data: MinerData { script_public_key, extra_data } })
    }

    pub fn calc_block_subsidy(&self, daa_score: u64) -> u64 {
        if daa_score < self.deflationary_phase_daa_score {
            return self.pre_deflationary_phase_base_subsidy;
        }

        let subsidy_month = self.subsidy_month(daa_score) as usize;
        let subsidy_table = if self.bps.activation().is_active(daa_score) {
            &self.subsidy_by_month_table_after
        } else {
            &self.subsidy_by_month_table_before
        };
        subsidy_table[subsidy_month.min(subsidy_table.len() - 1)]
    }

    /// Get the subsidy month as function of the current DAA score.
    ///
    /// Note that this function is called only if daa_score >= self.deflationary_phase_daa_score
    fn subsidy_month(&self, daa_score: u64) -> u64 {
        let seconds_since_deflationary_phase_started = if self.crescendo_activation_daa_score < self.deflationary_phase_daa_score {
            // crescendo_activation < deflationary_phase <= daa_score (activated before deflation)
            (daa_score - self.deflationary_phase_daa_score) / self.bps.after()
        } else if daa_score < self.crescendo_activation_daa_score {
            // deflationary_phase <= daa_score < crescendo_activation (pre activation)
            (daa_score - self.deflationary_phase_daa_score) / self.bps.before()
        } else {
            // Else - deflationary_phase <= crescendo_activation <= daa_score.
            // Count seconds differently before and after Crescendo activation
            (self.crescendo_activation_daa_score - self.deflationary_phase_daa_score) / self.bps.before()
                + (daa_score - self.crescendo_activation_daa_score) / self.bps.after()
        };

        seconds_since_deflationary_phase_started / SECONDS_PER_MONTH
    }

    #[cfg(test)]
    pub fn legacy_calc_block_subsidy(&self, daa_score: u64) -> u64 {
        if daa_score < self.deflationary_phase_daa_score {
            return self.pre_deflationary_phase_base_subsidy;
        }

        // Note that this calculation implicitly assumes that block per second = 1 (by assuming daa score diff is in second units).
        let months_since_deflationary_phase_started = (daa_score - self.deflationary_phase_daa_score) / SECONDS_PER_MONTH;
        assert!(months_since_deflationary_phase_started <= usize::MAX as u64);
        let months_since_deflationary_phase_started: usize = months_since_deflationary_phase_started as usize;
        if months_since_deflationary_phase_started >= SUBSIDY_BY_MONTH_TABLE.len() {
            *SUBSIDY_BY_MONTH_TABLE.last().unwrap()
        } else {
            SUBSIDY_BY_MONTH_TABLE[months_since_deflationary_phase_started]
        }
    }
}

/*
    SEDRA: the first 192 entries are sedrad v2.0.0's `subsidyByDeflationaryMonthTable` (changed upstream on 2025-04-08, e61214d);
    the rest is zero padding. Go clamps to its last entry (0), so the padding gives identical results.
    Original note: this table was pre-calculated by calling `calcDeflationaryPeriodBlockSubsidyFloatCalc` (in kaspad-go) for all months until reaching 0 subsidy.
    To regenerate this table, run `TestBuildSubsidyTable` in coinbasemanager_test.go (note the `deflationaryPhaseBaseSubsidy` therein).
    These values represent the reward per second for each month (= reward per block for 1 BPS).
*/
#[rustfmt::skip]
const SUBSIDY_BY_MONTH_TABLE: [u64; 426] = [
	44000000000, 41530469757, 39199543598, 36999442271, 34922823143, 32962755691, 31112698372, 29366476791, 17000000000, 15972745753, 15007565111, 14100707171, 13248647681, 12448075352, 11695878983, 10989135374, 10325097963, 9701186155, 9114975292, 8564187228, 8046681481, 7560446908, 7103593898, 6674347014, 6271038115,
	5892099848, 5536059578, 5201533653, 4887222034, 4591903234, 4314429581, 4053722751, 3808769580, 3578618126, 3362373969, 3159196737, 2968296839, 2788932396, 2620406358, 2462063796, 2313289355, 2173504866, 2042167095, 1918765635, 1802820920, 1693882365, 1591526609, 1495355875, 1404996424, 1320097098,
	1240327960, 1165379009, 1094958977, 1028794197, 966627538, 908217406, 853336807, 801772461, 753323982, 707803084, 665032865, 624847110, 587089648, 551613746, 518281537, 486963483, 457537877, 429890364, 403913499, 379506331, 356574008, 335027410, 314782802, 295761510, 277889612,
	261097653, 245320379, 230496473, 216568327, 203481813, 191186075, 179633327, 168778674, 158579933, 148997467, 139994039, 131534658, 123586449, 116118527, 109101865, 102509197, 96314902, 90494909, 85026598, 79888720, 75061306, 70525598, 66263967, 62259853, 58497694,
	54962870, 51641644, 48521109, 45589138, 42834336, 40245997, 37814064, 35529084, 33382178, 31365001, 29469718, 27688960, 26015807, 24443757, 22966701, 21578898, 20274956, 19049807, 17898690, 16817131, 15800927, 14846128, 13949026, 13106132, 12314171,
	11570066, 10870925, 10214031, 9596830, 9016925, 8472062, 7960122, 7479118, 7027180, 6602551, 6203580, 5828718, 5476508, 5145581, 4834650, 4542508, 4268019, 4010117, 3767799, 3540123, 3326205, 3125213, 2936367, 2758932, 2592219,
	2435579, 2288405, 2150124, 2020199, 1898125, 1783428, 1675661, 1574407, 1479270, 1389883, 1305897, 1226986, 1152843, 1083176, 1017712, 956187, 898687, 844049, 792052, 742491, 695179, 649941, 606616, 565050, 525098,
	486622, 449494, 413591, 378799, 345011, 312126, 280050, 248696, 217982, 187828, 158159, 128903, 99989, 71349, 43422, 16152, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MAINNET_PARAMS;
    use kaspa_consensus_core::{
        config::params::{ForkActivation, Params, SIMNET_PARAMS},
        constants::SOMPI_PER_KASPA,
        network::{NetworkId, NetworkType},
        tx::scriptvec,
    };

    #[test]
    fn calc_high_bps_total_rewards_delta() {
        let legacy_cbm = create_legacy_manager();
        let pre_deflationary_rewards = legacy_cbm.pre_deflationary_phase_base_subsidy * legacy_cbm.deflationary_phase_daa_score;
        let total_rewards: u64 = pre_deflationary_rewards + SUBSIDY_BY_MONTH_TABLE.iter().map(|x| x * SECONDS_PER_MONTH).sum::<u64>();
        let testnet_11_bps = SIMNET_PARAMS.bps().upper_bound();
        let total_high_bps_rewards_rounded_up: u64 = pre_deflationary_rewards
            + SUBSIDY_BY_MONTH_TABLE.iter().map(|x| (x.div_ceil(testnet_11_bps) * testnet_11_bps) * SECONDS_PER_MONTH).sum::<u64>();

        let cbm = create_manager(&SIMNET_PARAMS);
        let total_high_bps_rewards: u64 = pre_deflationary_rewards
            + cbm.subsidy_by_month_table_before.iter().map(|x| x * SECONDS_PER_MONTH * cbm.bps().before()).sum::<u64>();
        assert_eq!(total_high_bps_rewards_rounded_up, total_high_bps_rewards, "subsidy adjusted to bps must be rounded up");

        let delta = total_high_bps_rewards as i64 - total_rewards as i64;

        println!("Total rewards: {} sompi => {} KAS", total_rewards, total_rewards / SOMPI_PER_KASPA);
        println!("Total high bps rewards: {} sompi => {} KAS", total_high_bps_rewards, total_high_bps_rewards / SOMPI_PER_KASPA);
        println!("Delta: {} sompi => {} KAS", delta, delta / SOMPI_PER_KASPA as i64);
    }

    #[test]
    fn subsidy_by_month_table_test() {
        let cbm = create_legacy_manager();
        cbm.subsidy_by_month_table_before.iter().enumerate().for_each(|(i, x)| {
            assert_eq!(SUBSIDY_BY_MONTH_TABLE[i], *x, "for 1 BPS, const table and precomputed values must match");
        });

        for network_id in NetworkId::iter() {
            let cbm = create_manager(&network_id.into());
            cbm.subsidy_by_month_table_before.iter().enumerate().for_each(|(i, x)| {
                assert_eq!(
                    SUBSIDY_BY_MONTH_TABLE[i].div_ceil(cbm.bps().before()),
                    *x,
                    "{}: locally computed and precomputed values must match",
                    network_id
                );
            });
            cbm.subsidy_by_month_table_after.iter().enumerate().for_each(|(i, x)| {
                assert_eq!(
                    SUBSIDY_BY_MONTH_TABLE[i].div_ceil(cbm.bps().after()),
                    *x,
                    "{}: locally computed and precomputed values must match",
                    network_id
                );
            });
        }
    }

    /// Takes over 60 seconds, run with the following command line:
    /// `cargo test --release --package kaspa-consensus --lib -- processes::coinbase::tests::verify_crescendo_emission_schedule --exact --nocapture --ignored`
    #[test]
    #[ignore = "long"]
    fn verify_crescendo_emission_schedule() {
        // No need to loop over all nets since the relevant params are only
        // deflation and activation DAA scores (and the test is long anyway)
        for network_id in [NetworkId::new(NetworkType::Mainnet)] {
            let mut params: Params = network_id.into();
            params.crescendo_activation = ForkActivation::never();
            let cbm = create_manager(&params);
            let (baseline_epochs, baseline_total) = calculate_emission(cbm);

            let mut activations = vec![10000, 33444444, 120727479];
            for network_id in NetworkId::iter() {
                let activation = Params::from(network_id).crescendo_activation;
                if activation != ForkActivation::never() && activation != ForkActivation::always() {
                    activations.push(activation.daa_score());
                }
            }

            // Loop over a few random activation points + specified activation points for all nets
            for activation in activations {
                params.crescendo_activation = ForkActivation::new(activation);
                let cbm = create_manager(&params);
                let (new_epochs, new_total) = calculate_emission(cbm);

                // Epochs only represents the number of times the subsidy changed (lower after activation due to rounding)
                println!("BASELINE:\t{}\tepochs, total emission: {}", baseline_epochs, baseline_total);
                println!("CRESCENDO:\t{}\tepochs, total emission: {}, activation: {}", new_epochs, new_total, activation);

                let diff = (new_total as i64 - baseline_total as i64) / SOMPI_PER_KASPA as i64;
                assert!(diff.abs() <= 51, "activation: {}", activation);
                println!("DIFF (KAS): {}", diff);
            }
        }
    }

    fn calculate_emission(cbm: CoinbaseManager) -> (u64, u64) {
        let activation = cbm.bps().activation().daa_score();
        let mut current = 0;
        let mut total = 0;
        let mut epoch = 0u64;
        let mut prev = cbm.calc_block_subsidy(0);
        loop {
            let subsidy = cbm.calc_block_subsidy(current);
            // Pre activation we expect the legacy calc (1bps)
            if current < activation {
                assert_eq!(cbm.legacy_calc_block_subsidy(current), subsidy);
            }
            if subsidy == 0 {
                break;
            }
            total += subsidy;
            if subsidy != prev {
                println!("epoch: {}, subsidy: {}", epoch, subsidy);
                prev = subsidy;
                epoch += 1;
            }
            current += 1;
        }

        (epoch, total)
    }

    #[test]
    fn subsidy_test() {
        const PRE_DEFLATIONARY_PHASE_BASE_SUBSIDY: u64 = 50000000000;
        const DEFLATIONARY_PHASE_INITIAL_SUBSIDY: u64 = 44000000000;
        const SECONDS_PER_MONTH: u64 = 2629800;

        for network_id in NetworkId::iter() {
            let mut params: Params = network_id.into();
            if params.crescendo_activation != ForkActivation::always() {
                // We test activation scenarios in verify_crescendo_emission_schedule
                params.crescendo_activation = ForkActivation::never();
            }
            let cbm = create_manager(&params);
            let bps = params.bps().before();

            let pre_deflationary_phase_base_subsidy = PRE_DEFLATIONARY_PHASE_BASE_SUBSIDY / bps;
            let deflationary_phase_initial_subsidy = DEFLATIONARY_PHASE_INITIAL_SUBSIDY / bps;
            let blocks_per_month = SECONDS_PER_MONTH * bps;

            struct Test {
                name: &'static str,
                daa_score: u64,
                expected: u64,
            }

            // SEDRA: expected values are taken from sedrad v2.0.0's Go table (`subsidyByDeflationaryMonthTable`), not from this file.
            let tests = vec![
                Test { name: "first mined block", daa_score: 1, expected: pre_deflationary_phase_base_subsidy },
                Test {
                    name: "before deflationary phase",
                    daa_score: params.deflationary_phase_daa_score - 1,
                    expected: pre_deflationary_phase_base_subsidy,
                },
                Test {
                    name: "start of deflationary phase",
                    daa_score: params.deflationary_phase_daa_score,
                    expected: deflationary_phase_initial_subsidy,
                },
                Test { name: "month 7, last month before Sedra's step", daa_score: params.deflationary_phase_daa_score + 8 * blocks_per_month - 1, expected: (29366476791_u64).div_ceil(bps) },
                Test { name: "month 8, Sedra's step to 170 SDR", daa_score: params.deflationary_phase_daa_score + 8 * blocks_per_month, expected: (17000000000_u64).div_ceil(bps) },
                Test { name: "month 12", daa_score: params.deflationary_phase_daa_score + 12 * blocks_per_month, expected: (13248647681_u64).div_ceil(bps) },
                Test { name: "month 17 (live mainnet reward on 2026-11-05: 97.01186155 SDR)", daa_score: params.deflationary_phase_daa_score + 17 * blocks_per_month, expected: (9701186155_u64).div_ceil(bps) },
                Test { name: "month 190, last non-zero month", daa_score: params.deflationary_phase_daa_score + 190 * blocks_per_month, expected: (16152_u64).div_ceil(bps) },
                Test { name: "month 191, subsidy depleted", daa_score: params.deflationary_phase_daa_score + 191 * blocks_per_month, expected: 0 },
                Test { name: "far beyond the table", daa_score: params.deflationary_phase_daa_score + 1000 * blocks_per_month, expected: 0 },
            ];

            for t in tests {
                assert_eq!(cbm.calc_block_subsidy(t.daa_score), t.expected, "{} test '{}' failed", network_id, t.name);
                if bps == 1 {
                    assert_eq!(cbm.legacy_calc_block_subsidy(t.daa_score), t.expected, "{} test '{}' failed", network_id, t.name);
                }
            }
        }
    }

    #[test]
    fn payload_serialization_test() {
        let cbm = create_manager(&MAINNET_PARAMS);

        let script_data = [33u8, 255];
        let extra_data = [2u8, 3];
        let data = CoinbaseData {
            blue_score: 56,
            subsidy: 44000000000,
            miner_data: MinerData {
                script_public_key: ScriptPublicKey::new(0, ScriptVec::from_slice(&script_data)),
                extra_data: &extra_data as &[u8],
            },
        };

        let payload = cbm.serialize_coinbase_payload(&data).unwrap();
        let deserialized_data = cbm.deserialize_coinbase_payload(&payload).unwrap();

        assert_eq!(data, deserialized_data);

        // Test an actual mainnet payload
        let payload_hex =
            "b612c90100000000041a763e07000000000022202b32443ff740012157716d81216d09aebc39e5493c93a7181d92cb756c02c560ac302e31322e382f";
        let mut payload = vec![0u8; payload_hex.len() / 2];
        faster_hex::hex_decode(payload_hex.as_bytes(), &mut payload).unwrap();
        let deserialized_data = cbm.deserialize_coinbase_payload(&payload).unwrap();

        let expected_data = CoinbaseData {
            blue_score: 29954742,
            subsidy: 31112698372,
            miner_data: MinerData {
                script_public_key: ScriptPublicKey::new(
                    0,
                    scriptvec![
                        32, 43, 50, 68, 63, 247, 64, 1, 33, 87, 113, 109, 129, 33, 109, 9, 174, 188, 57, 229, 73, 60, 147, 167, 24,
                        29, 146, 203, 117, 108, 2, 197, 96, 172,
                    ],
                ),
                extra_data: &[48u8, 46, 49, 50, 46, 56, 47] as &[u8],
            },
        };
        assert_eq!(expected_data, deserialized_data);
    }

    #[test]
    fn modify_payload_test() {
        let cbm = create_manager(&MAINNET_PARAMS);

        let script_data = [33u8, 255];
        let extra_data = [2u8, 3, 23, 98];
        let data = CoinbaseData {
            blue_score: 56345,
            subsidy: 44000000000,
            miner_data: MinerData {
                script_public_key: ScriptPublicKey::new(0, ScriptVec::from_slice(&script_data)),
                extra_data: &extra_data,
            },
        };

        let data2 = CoinbaseData {
            blue_score: data.blue_score,
            subsidy: data.subsidy,
            miner_data: MinerData {
                // Modify only miner data
                script_public_key: ScriptPublicKey::new(0, ScriptVec::from_slice(&[33u8, 255, 33])),
                extra_data: &[2u8, 3, 23, 98, 34, 34] as &[u8],
            },
        };

        let mut payload = cbm.serialize_coinbase_payload(&data).unwrap();
        payload = cbm.modify_coinbase_payload(payload, &data2.miner_data).unwrap(); // Update the payload with the modified miner data
        let deserialized_data = cbm.deserialize_coinbase_payload(&payload).unwrap();

        assert_eq!(data2, deserialized_data);
    }

    /// SEDRA (M2): subsidy parity with the Go node. The vectors were produced by sedrad v2.0.0's own
    /// `coinbasemanager.CalcBlockSubsidy` with mainnet params (only the DAA score lookup was stubbed), for the first
    /// blocks, the start of the deflationary phase, and one block before, at and after every month boundary up to
    /// month 200, plus mid-month points and selected mainnet DAA scores. Generator: research archive of the project.
    #[test]
    fn sedra_subsidy_parity_with_go_test() {
        let doc: serde_json::Value = serde_json::from_str(include_str!("testdata/sedrad_subsidy_vectors.json")).unwrap();
        let params = MAINNET_PARAMS;
        assert_eq!(doc["deflationary_phase_daa_score"].as_u64().unwrap(), params.deflationary_phase_daa_score);
        assert_eq!(doc["pre_deflationary_phase_subsidy"].as_u64().unwrap(), params.pre_deflationary_phase_base_subsidy);
        let cbm = create_manager(&params);
        let vectors = doc["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 813);
        for v in vectors {
            let daa_score = v["daa_score"].as_u64().unwrap();
            let expected = v["subsidy"].as_u64().unwrap();
            assert_eq!(cbm.calc_block_subsidy(daa_score), expected, "calc_block_subsidy at DAA score {daa_score}");
            assert_eq!(cbm.legacy_calc_block_subsidy(daa_score), expected, "legacy_calc_block_subsidy at DAA score {daa_score}");
        }
    }

    fn create_manager(params: &Params) -> CoinbaseManager {
        CoinbaseManager::new(
            params.coinbase_payload_script_public_key_max_len,
            params.max_coinbase_payload_len,
            params.deflationary_phase_daa_score,
            params.pre_deflationary_phase_base_subsidy,
            params.bps(),
        )
    }

    /// Return a CoinbaseManager with legacy golang 1 BPS properties
    fn create_legacy_manager() -> CoinbaseManager {
        CoinbaseManager::new(150, 204, 15778800 - 259200, 50000000000, ForkedParam::new_const(1))
    }
}
