#![no_std]

// StreamFactory — Issue #237
//
// An on-chain registry and factory that deploys child `SoroStreamContract`
// instances at deterministic addresses using a caller-supplied salt.
//
// ## Design
//
// * A single privileged `admin` is stored during `initialize`.
// * `deploy_stream_contract(admin, wasm_hash, salt)` deploys a new stream
//   contract instance deterministically, records deployment metadata, and
//   returns the new contract's `Address`.
// * `get_deployed_contracts(start, limit)` returns a bounded page of child addresses.
// * `get_deployment_info` returns metadata for a single child contract.
//
// ## Storage layout
//
// | Key                        | Type           | Description                        |
// |----------------------------|----------------|------------------------------------|
// | `Symbol("admin")`          | `Address`      | Factory admin                      |
// | `Symbol("contract_count")` | `u32`          | Number of deployed addresses       |
// | `(Symbol("contract"), u32)` | `Address`     | Address at a deployment index       |
// | `(Symbol("info"), Address)` | `DeploymentInfo` | Per-contract metadata            |

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metadata stored for each deployed stream contract.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DeploymentInfo {
    /// Address of the deployed child contract.
    pub contract_address: Address,
    /// Address of the account that triggered the deployment.
    pub deployer: Address,
    /// Ledger sequence number at time of deployment.
    pub deploy_ledger: u32,
    /// Unix timestamp at time of deployment.
    pub deploy_timestamp: u64,
    /// WASM hash used for this deployment (encodes the contract version).
    pub wasm_hash: BytesN<32>,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

const ADMIN_KEY: &str = "admin";
const PENDING_ADMIN_KEY: &str = "pending_admin";
const CONTRACT_COUNT_KEY: &str = "contract_count";
const CONTRACT_PREFIX: &str = "contract";
const INFO_PREFIX: &str = "info";
const MAX_DEPLOYMENT_PAGE_SIZE: u32 = 20;

fn admin_key(env: &Env) -> Symbol {
    Symbol::new(env, ADMIN_KEY)
}

fn contract_index_key(env: &Env, index: u32) -> (Symbol, u32) {
    (Symbol::new(env, CONTRACT_PREFIX), index)
}

fn info_key(env: &Env, addr: &Address) -> (Symbol, Address) {
    (Symbol::new(env, INFO_PREFIX), addr.clone())
}

fn read_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&admin_key(env))
}

fn read_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&Symbol::new(env, PENDING_ADMIN_KEY))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FactoryError {
    /// Factory has already been initialised.
    AlreadyInitialized = 1,
    /// Caller is not the factory admin.
    Unauthorized = 2,
    /// Factory has not been initialised yet.
    NotInitialized = 3,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct StreamFactory;

#[contractimpl]
impl StreamFactory {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initializes the factory with an `admin` address.
    ///
    /// Must be called exactly once after deployment. Subsequent calls revert
    /// with `AlreadyInitialized`.
    ///
    /// # Arguments
    /// * `admin` - Address that will be permitted to deploy child contracts.
    pub fn initialize(env: Env, admin: Address) -> Result<(), FactoryError> {
        if env.storage().instance().has(&admin_key(&env)) {
            return Err(FactoryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&admin_key(&env), &admin);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Deployment
    // -----------------------------------------------------------------------

    /// Deploys a child `SoroStreamContract` at a deterministic address derived
    /// from `salt` and records its metadata in the on-chain registry.
    ///
    /// Only the factory admin may call this function.
    ///
    /// # Arguments
    /// * `admin` - Must match the stored admin address (auth required).
    /// * `wasm_hash` - SHA-256 hash of the child contract WASM (previously
    ///   uploaded via `env.deployer().upload_contract_wasm()`).
    /// * `salt` - 32-byte salt used to derive the deterministic address.
    ///
    /// # Returns
    /// The `Address` of the newly deployed child contract.
    pub fn deploy_stream_contract(
        env: Env,
        admin: Address,
        wasm_hash: BytesN<32>,
        salt: BytesN<32>,
    ) -> Result<Address, FactoryError> {
        // Auth + admin check.
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key(&env))
            .ok_or(FactoryError::NotInitialized)?;
        if admin != stored_admin {
            return Err(FactoryError::Unauthorized);
        }

        // Deploy child contract at deterministic address.
        let child_address = env
            .deployer()
            .with_current_contract(salt)
            .deploy_v2(wasm_hash.clone(), ());

        // Build and persist deployment metadata.
        let info = DeploymentInfo {
            contract_address: child_address.clone(),
            deployer: admin.clone(),
            deploy_ledger: env.ledger().sequence(),
            deploy_timestamp: env.ledger().timestamp(),
            wasm_hash: wasm_hash.clone(),
        };
        env.storage()
            .persistent()
            .set(&info_key(&env, &child_address), &info);

        // Store each address separately so append and paginated reads stay bounded.
        let count: u32 = env
            .storage()
            .persistent()
            .get(&Symbol::new(&env, CONTRACT_COUNT_KEY))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&contract_index_key(&env, count), &child_address);
        env.storage().persistent().set(
            &Symbol::new(&env, CONTRACT_COUNT_KEY),
            &count.checked_add(1).expect("deployment count overflow"),
        );

        env.events().publish(
            (Symbol::new(&env, "StreamContractDeployed"), child_address.clone()),
            (admin, wasm_hash),
        );

        Ok(child_address)
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    /// Returns a page of deployed child contract addresses in deployment order.
    ///
    /// `start` is the zero-based deployment index and `limit` is capped at 20.
    pub fn get_deployed_contracts(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let count: u32 = env.storage()
            .persistent()
            .get(&Symbol::new(&env, CONTRACT_COUNT_KEY))
            .unwrap_or(0);
        let end = start
            .saturating_add(limit.min(MAX_DEPLOYMENT_PAGE_SIZE))
            .min(count);
        let mut page = Vec::new(&env);

        for index in start..end {
            if let Some(address) = env
                .storage()
                .persistent()
                .get(&contract_index_key(&env, index))
            {
                page.push_back(address);
            }
        }

        page
    }

    /// Returns deployment metadata for a single child contract address.
    ///
    /// Returns `None` if the address was not deployed through this factory.
    pub fn get_deployment_info(env: Env, contract_address: Address) -> Option<DeploymentInfo> {
        env.storage()
            .persistent()
            .get(&info_key(&env, &contract_address))
    }

    /// Returns the current factory admin address.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&admin_key(&env))
    }

    /// Proposes a new admin. Only the current admin may call this.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), FactoryError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&admin_key(&env))
            .ok_or(FactoryError::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&Symbol::new(&env, PENDING_ADMIN_KEY), &new_admin);
        env.events().publish(
            (Symbol::new(&env, "AdminTransferProposed"),),
            new_admin,
        );
        Ok(())
    }

    /// Accepts the admin role. The pending admin must call this.
    pub fn accept_admin(env: Env, accepted_by: Address) -> Result<(), FactoryError> {
        accepted_by.require_auth();
        let pending = read_pending_admin(&env)
            .ok_or(FactoryError::NotInitialized)?;
        if accepted_by != pending {
            return Err(FactoryError::Unauthorized);
        }
        let current_admin: Address = env
            .storage()
            .instance()
            .get(&admin_key(&env))
            .ok_or(FactoryError::NotInitialized)?;
        env.storage()
            .instance()
            .set(&admin_key(&env), &accepted_by);
        env.storage()
            .instance()
            .remove(&Symbol::new(&env, PENDING_ADMIN_KEY));
        env.events().publish(
            (Symbol::new(&env, "AdminTransferAccepted"),),
            (current_admin, accepted_by),
        );
        Ok(())
    }
}
