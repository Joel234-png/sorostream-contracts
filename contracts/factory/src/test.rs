#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fake_salt(env: &Env, n: u8) -> BytesN<32> {
    BytesN::from_array(env, &[n; 32])
}

/// Upload a valid Soroban WASM fixture and return its hash.
/// Uses a minimal pre-built WASM that the Soroban host accepts.
fn upload_child_wasm(env: &Env) -> BytesN<32> {
    const CHILD_WASM: &[u8] = soroban_sdk::contractfile!(
        file = "test_fixtures/child_contract.wasm",
        sha256 = "fd41d2f77920ca07b723e05f732a82db4c2f6459eb2be6b40c4f225434569550"
    );
    env.deployer().upload_contract_wasm(CHILD_WASM)
}

// ---------------------------------------------------------------------------
// Tests — init and auth only (no WASM required)
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_stores_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);

    c.initialize(&admin);
    assert_eq!(c.get_admin(), Some(admin));
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);

    c.initialize(&admin);
    let result = c.try_initialize(&admin);
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, FactoryError::AlreadyInitialized),
        _ => panic!("expected AlreadyInitialized"),
    }
}

#[test]
fn test_get_deployed_contracts_empty_before_any_deployment() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);
    c.initialize(&admin);

    assert_eq!(c.get_deployed_contracts().len(), 0);
}

#[test]
fn test_get_admin_returns_none_before_init() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);

    assert_eq!(c.get_admin(), None);
}

#[test]
fn test_deploy_not_initialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);

    // Call deploy before initialize — must fail with NotInitialized.
    let result = c.try_deploy_stream_contract(
        &admin,
        &BytesN::from_array(&env, &[0xAB_u8; 32]),
        &fake_salt(&env, 1),
    );
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, FactoryError::NotInitialized),
        _ => panic!("expected NotInitialized"),
    }
}

#[test]
fn test_deploy_unauthorized_non_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    c.initialize(&admin);

    // attacker ≠ stored admin → Unauthorized
    let result = c.try_deploy_stream_contract(
        &attacker,
        &BytesN::from_array(&env, &[0xAB_u8; 32]),
        &fake_salt(&env, 1),
    );
    assert!(result.is_err());
    match result {
        Err(Ok(e)) => assert_eq!(e, FactoryError::Unauthorized),
        _ => panic!("expected Unauthorized"),
    }
}

#[test]
fn test_get_deployment_info_unknown_address_returns_none() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);
    c.initialize(&admin);

    let random_addr = Address::generate(&env);
    assert!(c.get_deployment_info(&random_addr).is_none());
}

// ---------------------------------------------------------------------------
// Tests — deployment (require pre-built WASM)
// ---------------------------------------------------------------------------

#[test]
fn test_deploy_records_metadata_and_registry() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(42);
    env.ledger().set_timestamp(1000);

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);
    c.initialize(&admin);

    let wasm_hash = upload_child_wasm(&env);
    let child_addr = c.deploy_stream_contract(&admin, &wasm_hash, &fake_salt(&env, 1));

    // Registry has one entry.
    let contracts = c.get_deployed_contracts();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts.get(0).unwrap(), child_addr);

    // Metadata is correct.
    let info = c.get_deployment_info(&child_addr).unwrap();
    assert_eq!(info.deployer, admin);
    assert_eq!(info.deploy_ledger, 42);
    assert_eq!(info.deploy_timestamp, 1000);
    assert_eq!(info.wasm_hash, wasm_hash);
    assert_eq!(info.contract_address, child_addr);
}

#[test]
fn test_deployment_deployer_metadata_survives_admin_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let original_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    c.initialize(&original_admin);

    let wasm_hash = upload_child_wasm(&env);
    let first_child = c.deploy_stream_contract(&original_admin, &wasm_hash, &fake_salt(&env, 41));

    c.propose_admin(&new_admin);
    c.accept_admin(&new_admin);

    let first_info = c.get_deployment_info(&first_child).unwrap();
    assert_eq!(first_info.deployer, original_admin);

    let second_child = c.deploy_stream_contract(&new_admin, &wasm_hash, &fake_salt(&env, 42));
    let second_info = c.get_deployment_info(&second_child).unwrap();
    assert_eq!(second_info.deployer, new_admin);
}

#[test]
fn test_different_salts_give_different_addresses() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);
    c.initialize(&admin);

    let wasm_hash = upload_child_wasm(&env);

    let addr1 = c.deploy_stream_contract(&admin, &wasm_hash, &fake_salt(&env, 1));
    let addr2 = c.deploy_stream_contract(&admin, &wasm_hash, &fake_salt(&env, 2));

    // Different salts must produce different child addresses.
    assert_ne!(addr1, addr2);
}

#[test]
fn test_deploy_multiple_contracts_all_listed_in_order() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(StreamFactory, ());
    let c = StreamFactoryClient::new(&env, &factory_id);
    let admin = Address::generate(&env);
    c.initialize(&admin);

    let wasm_hash = upload_child_wasm(&env);

    let addr1 = c.deploy_stream_contract(&admin, &wasm_hash, &fake_salt(&env, 10));
    let addr2 = c.deploy_stream_contract(&admin, &wasm_hash, &fake_salt(&env, 20));
    let addr3 = c.deploy_stream_contract(&admin, &wasm_hash, &fake_salt(&env, 30));

    let contracts = c.get_deployed_contracts();
    assert_eq!(contracts.len(), 3);
    assert_eq!(contracts.get(0).unwrap(), addr1);
    assert_eq!(contracts.get(1).unwrap(), addr2);
    assert_eq!(contracts.get(2).unwrap(), addr3);
}
