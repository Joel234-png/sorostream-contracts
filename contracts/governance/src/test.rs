use super::*;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger, LedgerInfo},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal,
};

#[contract]
struct TargetContract;

#[contractimpl]
impl TargetContract {
    pub fn set_value(env: Env, value: u32) -> Result<(), GovernanceError> {
        env.storage().instance().set(&Symbol::new(&env, "value"), &value);
        Ok(())
    }

    pub fn value(env: Env) -> u32 {
        env.storage().instance().get(&Symbol::new(&env, "value")).unwrap_or(0)
    }
}

struct TestEnv {
    env: Env,
    voter: Address,
    token: Address,
    governance: Address,
}

impl TestEnv {
    fn new(voter_balance: i128) -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let guardian = Address::generate(&env);
        let voter = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = env.register_stellar_asset_contract_v2(token_admin).address();
        if voter_balance > 0 {
            StellarAssetClient::new(&env, &token).mint(&voter, &voter_balance);
        }

        let governance = env.register(GovernanceContract, ());
        GovernanceContractClient::new(&env, &governance).initialize(
            &admin, &guardian, &token, &1, &1, &10, &5,
        );

        Self { env, voter, token, governance }
    }

    fn governance_client(&self) -> GovernanceContractClient {
        GovernanceContractClient::new(&self.env, &self.governance)
    }

    fn set_time(&self, timestamp: u64) {
        self.env.ledger().set(LedgerInfo {
            timestamp,
            ..self.env.ledger().get()
        });
    }
}

#[test]
fn rejects_proposer_balance_above_u64() {
    let t = TestEnv::new((u64::MAX as i128) + 1);
    let target = t.env.register(TargetContract, ());
    let result = t.governance_client().try_create_proposal(
        &t.voter,
        &target,
        &Symbol::new(&t.env, "set_value"),
        &soroban_sdk::Vec::new(&t.env),
    );

    assert!(matches!(result, Ok(Err(GovernanceError::InvalidParam))));
}

#[test]
fn rejects_voter_balance_above_u64() {
    let t = TestEnv::new(10);
    let target = t.env.register(TargetContract, ());
    let proposal_id = t.governance_client().create_proposal(
        &t.voter,
        &target,
        &Symbol::new(&t.env, "set_value"),
        &soroban_sdk::Vec::new(&t.env),
    );
    StellarAssetClient::new(&t.env, &t.token)
        .mint(&t.voter, &((u64::MAX as i128) + 1));

    let result = t.governance_client().try_vote(&t.voter, &proposal_id, &true);
    assert!(matches!(result, Ok(Err(GovernanceError::InvalidParam))));
}

#[test]
fn vote_escrows_tokens_and_execute_passes_typed_arguments() {
    let t = TestEnv::new(100);
    let target = t.env.register(TargetContract, ());
    let args = soroban_sdk::vec![&t.env, 42u32.into_val(&t.env)];
    let proposal_id = t.governance_client().create_proposal(
        &t.voter,
        &target,
        &Symbol::new(&t.env, "set_value"),
        &args,
    );

    t.governance_client().vote(&t.voter, &proposal_id, &true);

    let token_client = TokenClient::new(&t.env, &t.token);
    assert_eq!(token_client.balance(&t.voter), 0);
    assert_eq!(token_client.balance(&t.governance), 100);

    t.set_time(11);
    t.governance_client().queue(&proposal_id);
    t.set_time(16);
    t.governance_client().execute(&proposal_id);

    assert_eq!(TargetContractClient::new(&t.env, &target).value(), 42);

    t.governance_client()
        .withdraw_vote_tokens(&t.voter, &proposal_id);
    assert_eq!(token_client.balance(&t.voter), 100);
    assert_eq!(token_client.balance(&t.governance), 0);
}