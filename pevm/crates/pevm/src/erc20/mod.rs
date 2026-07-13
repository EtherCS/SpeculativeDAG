//! ERC-20 testing module

/// This module provides ERC-20 contract functionality.
pub mod contract;

use crate::{Bytecodes, ChainState, EvmAccount};
use contract::ERC20Token;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use revm::primitives::{uint, Address, TransactTo, TxEnv, U256};

/// The maximum amount of gas that can be used for a transaction in this configuration.
pub const GAS_LIMIT: u64 = 35_000;

/// An estimated amount of gas that is expected to be consumed by typical transactions.
pub const ESTIMATED_GAS_USED: u64 = 29_738;

// TODO: Better randomness control.
/// Sometimes we want duplicates to test
/// dependent transactions, sometimes we want to guarantee non-duplicates
/// for independent benchmarks.
fn generate_addresses(length: usize) -> Vec<Address> {
    (0..length).map(|_| Address::new(rand::random())).collect()
}

fn deterministic_address(rng: &mut StdRng) -> Address {
    let mut bytes = [0u8; 20];
    rng.fill_bytes(&mut bytes);
    Address::new(bytes)
}

/// Generates a cluster of blockchain transactions for testing or simulation purposes.
pub fn generate_cluster(
    num_families: usize,
    num_people_per_family: usize,
    num_transfers_per_person: usize,
) -> (ChainState, Bytecodes, Vec<TxEnv>) {
    let families: Vec<Vec<Address>> = (0..num_families)
        .map(|_| generate_addresses(num_people_per_family))
        .collect();

    let people_addresses: Vec<Address> = families.clone().into_iter().flatten().collect();

    let gld_address = Address::new(rand::random());

    let gld_account = ERC20Token::new("Gold Token", "GLD", 18, 222_222_000_000_000_000_000_000u128)
        .add_balances(&people_addresses, uint!(1_000_000_000_000_000_000_U256))
        .build();

    let mut state = ChainState::from_iter([(gld_address, gld_account)]);
    let mut txs = Vec::new();

    for person in &people_addresses {
        state.insert(
            *person,
            EvmAccount {
                balance: uint!(4_567_000_000_000_000_000_000_U256),
                ..EvmAccount::default()
            },
        );
    }

    for nonce in 0..num_transfers_per_person {
        for family in &families {
            for person in family {
                let recipient = family[(rand::random::<usize>()) % (family.len())];
                let calldata = ERC20Token::transfer(recipient, U256::from(rand::random::<u8>()));

                txs.push(TxEnv {
                    caller: *person,
                    gas_limit: GAS_LIMIT,
                    gas_price: U256::from(0xb2d05e07u64),
                    transact_to: TransactTo::Call(gld_address),
                    data: calldata,
                    nonce: Some(nonce as u64),
                    ..TxEnv::default()
                })
            }
        }
    }

    let mut bytecodes = Bytecodes::default();
    for account in state.values_mut() {
        let code = account.code.take();
        if let Some(code) = code {
            bytecodes.insert(account.code_hash.unwrap(), code);
        }
    }

    (state, bytecodes, txs)
}

/// Generates a cluster of blockchain transactions for testing or simulation purposes.
pub fn generate_state_and_byte_code(
    num_families: usize,
    num_people_per_family: usize,
) -> (ChainState, Bytecodes, Address, Vec<Vec<Address>>) {
    generate_state_and_byte_code_with_seed(num_families, num_people_per_family, 0x4552_4332_30)
}

pub fn generate_state_and_byte_code_with_seed(
    num_families: usize,
    num_people_per_family: usize,
    seed: u64,
) -> (ChainState, Bytecodes, Address, Vec<Vec<Address>>) {
    let mut rng = StdRng::seed_from_u64(seed);

    let families: Vec<Vec<Address>> = (0..num_families)
        .map(|_| {
            (0..num_people_per_family)
                .map(|_| deterministic_address(&mut rng))
                .collect()
        })
        .collect();

    let people_addresses: Vec<Address> = families.clone().into_iter().flatten().collect();

    let gld_address = deterministic_address(&mut rng);

    let gld_account = ERC20Token::new("Gold Token", "GLD", 18, 222_222_000_000_000_000_000_000u128)
        .add_balances(&people_addresses, uint!(1_000_000_000_000_000_000_U256))
        .build();

    let mut state = ChainState::from_iter([(gld_address, gld_account)]);

    for person in &people_addresses {
        state.insert(
            *person,
            EvmAccount {
                balance: uint!(4_567_000_000_000_000_000_000_U256),
                ..EvmAccount::default()
            },
        );
    }

    let mut bytecodes = Bytecodes::default();
    for account in state.values_mut() {
        let code = account.code.take();
        if let Some(code) = code {
            bytecodes.insert(account.code_hash.unwrap(), code);
        }
    }

    (state, bytecodes, gld_address, families)
}
