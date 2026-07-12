//! WETH9 workload generation helpers.

pub mod contract;

use crate::{Bytecodes, ChainState, EvmAccount};
use contract::WETH9;
use rand::Rng;
use revm::primitives::{uint, Address};

fn generate_addresses(length: usize, rng: &mut impl Rng) -> Vec<Address> {
    (0..length).map(|_| Address::new(rng.gen())).collect()
}

pub fn generate_state_and_byte_code(
    num_families: usize,
    num_people_per_family: usize,
    rng: &mut impl Rng,
) -> (ChainState, Bytecodes, Address, Vec<Vec<Address>>) {
    assert!(
        num_people_per_family >= 2,
        "WETH workload needs at least one operator and one user per family"
    );

    let mut families = Vec::with_capacity(num_families);
    let mut operators = Vec::with_capacity(num_families);
    let mut users = Vec::new();

    for _ in 0..num_families {
        let family = generate_addresses(num_people_per_family, rng);
        operators.push(family[0]);
        users.extend(family.iter().copied().skip(1));
        families.push(family);
    }

    let weth_address = Address::new(rng.gen());
    let weth_account = WETH9::new("Wrapped Ether", "WETH", 18)
        .add_balances(&users, uint!(1_000_000_000_000_000_000_U256))
        .add_allowances_to_family_operators(&families, uint!(500_000_000_000_000_000_U256))
        .build();

    let mut state = ChainState::from_iter([(weth_address, weth_account)]);

    for address in operators.into_iter().chain(users.into_iter()) {
        state.insert(
            address,
            EvmAccount {
                balance: uint!(4_567_000_000_000_000_000_000_U256),
                ..EvmAccount::default()
            },
        );
    }

    let mut bytecodes = Bytecodes::default();
    for account in state.values_mut() {
        if let Some(code) = account.code.take() {
            bytecodes.insert(account.code_hash.unwrap(), code);
        }
    }

    (state, bytecodes, weth_address, families)
}
