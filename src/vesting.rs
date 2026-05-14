//! # Token Vesting Core — `vesting.rs`
//!
//! Minimal stub implementation that compiles cleanly under the current build.
//! The full vesting flow is disabled pending a re-implementation; only the
//! types, storage keys, and a handful of read-only helpers remain.

#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

// ── Storage keys ─────────────────────────────────────────────────────────────

/// Persistent storage keys for vesting state.
#[contracttype]
#[derive(Clone)]
pub enum VestingKey {
    /// The full [`VestingSchedule`] for a given beneficiary.
    Schedule(Address),
    /// How many tokens the beneficiary has already claimed.
    Claimed(Address),
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A single vesting tranche for a beneficiary.
#[contracttype]
#[derive(Clone)]
pub struct VestingSchedule {
    pub issuer: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub total_amount: i128,
    pub cliff_ts: u64,
    pub start_ts: u64,
    pub end_ts: u64,
}

/// Errors produced by the vesting module.
#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum VestingError {
    /// A schedule already exists for this beneficiary.
    ScheduleAlreadyExists = 100,
    /// No schedule found for the given beneficiary.
    ScheduleNotFound = 101,
    /// `total_amount` must be > 0.
    InvalidAmount = 102,
    /// Timestamp ordering violated.
    InvalidTimestamps = 103,
    /// Nothing to claim at the current ledger time.
    NothingToClaimYet = 104,
    /// Caller is not authorised for this operation.
    Unauthorized = 105,
}

/// Shared schema version for vesting events.
pub const VESTING_EVENT_SCHEMA_VERSION: u32 = 1;

// Legacy event symbols (for backward compatibility).
const EVENT_VESTING_CREATED: Symbol = symbol_short!("vest_crt");
const EVENT_VESTING_CLAIMED: Symbol = symbol_short!("vest_clm");

#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {
    /// Register a new vesting schedule for `beneficiary`.
    pub fn vesting_register(
        env: Env,
        issuer: Address,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        cliff_ts: u64,
        start_ts: u64,
        end_ts: u64,
    ) -> Result<(), VestingError> {
        issuer.require_auth();

        if total_amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }
        if start_ts < cliff_ts || end_ts <= start_ts {
            return Err(VestingError::InvalidTimestamps);
        }

        let key = VestingKey::Schedule(beneficiary.clone());
        if env.storage().persistent().has(&key) {
            return Err(VestingError::ScheduleAlreadyExists);
        }

        let schedule = VestingSchedule {
            issuer,
            beneficiary: beneficiary.clone(),
            token,
            total_amount,
            cliff_ts,
            start_ts,
            end_ts,
        };
        env.storage().persistent().set(&key, &schedule);
        env.storage().persistent().set(&VestingKey::Claimed(beneficiary.clone()), &0_i128);

        env.events().publish(
            (EVENT_VESTING_CREATED, beneficiary),
            (total_amount, cliff_ts, start_ts, end_ts),
        );

        Ok(())
    }

    /// Claim all tokens that have vested up to the current ledger timestamp.
    pub fn vesting_claim(env: Env, beneficiary: Address) -> Result<i128, VestingError> {
        beneficiary.require_auth();

        let sched_key = VestingKey::Schedule(beneficiary.clone());
        let claimed_key = VestingKey::Claimed(beneficiary.clone());

        let schedule: VestingSchedule =
            env.storage().persistent().get(&sched_key).ok_or(VestingError::ScheduleNotFound)?;

        let already_claimed: i128 = env.storage().persistent().get(&claimed_key).unwrap_or(0_i128);

        let now = env.ledger().timestamp();
        if now < schedule.cliff_ts {
            return Err(VestingError::NothingToClaimYet);
        }

        let claimable = compute_claimable(&schedule, already_claimed, now);
        if claimable == 0 {
            return Ok(0);
        }

        let new_claimed = already_claimed.saturating_add(claimable);
        env.storage().persistent().set(&claimed_key, &new_claimed);

        env.events().publish((EVENT_VESTING_CLAIMED, beneficiary), claimable);
        Ok(claimable)
    }

    /// Return the total tokens already claimed by `beneficiary`.
    pub fn get_claimed_amount(env: Env, beneficiary: Address) -> i128 {
        env.storage().persistent().get(&VestingKey::Claimed(beneficiary)).unwrap_or(0_i128)
    }

    /// Return the tokens vested (but not necessarily claimed) at the current
    /// ledger timestamp.
    pub fn get_vested_amount(env: Env, beneficiary: Address) -> Option<i128> {
        let schedule: VestingSchedule =
            env.storage().persistent().get(&VestingKey::Schedule(beneficiary))?;
        let now = env.ledger().timestamp();
        Some(compute_vested(&schedule, now))
    }

    /// Return the currently claimable amount for `beneficiary`.
    pub fn get_claimable_amount(env: Env, beneficiary: Address) -> Option<i128> {
        let schedule: VestingSchedule =
            env.storage().persistent().get(&VestingKey::Schedule(beneficiary.clone()))?;
        let claimed: i128 =
            env.storage().persistent().get(&VestingKey::Claimed(beneficiary)).unwrap_or(0_i128);
        let now = env.ledger().timestamp();
        Some(compute_claimable(&schedule, claimed, now))
    }

    /// Return all schedules for a batch of beneficiaries.
    pub fn get_vesting_schedules(
        env: Env,
        beneficiaries: Vec<Address>,
    ) -> Vec<Option<VestingSchedule>> {
        let mut out = Vec::new(&env);
        for b in beneficiaries.iter() {
            let s = env.storage().persistent().get(&VestingKey::Schedule(b));
            out.push_back(s);
        }
        out
    }
}

/// Helper: compute total vested tokens at a given timestamp.
fn compute_vested(schedule: &VestingSchedule, now: u64) -> i128 {
    if now < schedule.cliff_ts {
        return 0;
    }
    if now >= schedule.end_ts {
        return schedule.total_amount;
    }
    if now <= schedule.start_ts {
        return 0;
    }
    let elapsed = (now - schedule.start_ts) as i128;
    let duration = (schedule.end_ts - schedule.start_ts) as i128;
    if duration == 0 {
        return schedule.total_amount;
    }
    schedule.total_amount.checked_mul(elapsed).map(|m| m / duration).unwrap_or(0)
}

/// Helper: compute claimable tokens given prior claimed amount.
fn compute_claimable(schedule: &VestingSchedule, already_claimed: i128, now: u64) -> i128 {
    let vested = compute_vested(schedule, now);
    let claimable = vested.saturating_sub(already_claimed);
    if claimable < 0 {
        0
    } else {
        claimable
    }
}

use soroban_sdk::contracterror;
