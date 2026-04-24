#![cfg(test)]
// `setup_context` returns several values; callers may not use all of them.
// Suppress only the specific lint rather than silencing all warnings.
#![allow(dead_code)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Core test utilities avoiding self-referential struct lifetime errors.
pub fn setup_context() -> (Env, RevoraRevenueShareClient, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    (env, client, contract_id, issuer, token, payout_asset)
}
