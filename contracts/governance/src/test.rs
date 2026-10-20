#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Bytes, Env, Symbol,
};

#[test]
fn create_proposal_rejects_governance_contract_as_target() {
    let env = Env::default();
    env.mock_all_auths();
    let governance_id = env.register(GovernanceContract, ());
    let proposer = Address::generate(&env);
    let guardian = Address::generate(&env);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin).address();
    StellarAssetClient::new(&env, &token_id).mint(&proposer, &10);
    let client = GovernanceContractClient::new(&env, &governance_id);
    client.initialize(&admin, &guardian, &token_id, &1, &1, &10, &10);

    let result = client.try_create_proposal(
        &proposer,
        &governance_id,
        &Symbol::new(&env, "noop"),
        &Bytes::new(&env),
    );

    assert_eq!(result, Err(Ok(GovernanceError::InvalidParam)));
}

#[test]
fn execute_rejects_a_second_call_after_execution() {
    let env = Env::default();
    env.mock_all_auths();
    let governance_id = env.register(GovernanceContract, ());
    let target = Address::generate(&env);
    let proposer = Address::generate(&env);
    let guardian = Address::generate(&env);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin).address();
    StellarAssetClient::new(&env, &token_id).mint(&proposer, &10);
    let client = GovernanceContractClient::new(&env, &governance_id);
    client.initialize(&admin, &guardian, &token_id, &1, &1, &1, &1);

    let id = client.create_proposal(&proposer, &target, &Symbol::new(&env, "noop"), &Bytes::new(&env));
    client.vote(&proposer, &id, &true);
    env.ledger().set_timestamp(2);
    client.queue(&id);
    env.ledger().set_timestamp(4);
    let _ = client.try_execute(&id);
    let second = client.try_execute(&id);

    assert_eq!(second, Err(Ok(GovernanceError::ProposalNotQueued)));
}