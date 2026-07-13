//! WETH9 workload generation helpers.

pub mod contract;

use crate::{Bytecodes, ChainState, EvmAccount};
use contract::WETH9;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use revm::primitives::{uint, Address};

fn deterministic_address(rng: &mut StdRng) -> Address {
    let mut bytes = [0u8; 20];
    rng.fill_bytes(&mut bytes);
    Address::new(bytes)
}

pub fn generate_state_and_byte_code(
    num_families: usize,
    num_people_per_family: usize,
) -> (ChainState, Bytecodes, Address, Vec<Vec<Address>>) {
    generate_state_and_byte_code_with_seed(num_families, num_people_per_family, 0x5745_5448)
}

pub fn generate_state_and_byte_code_with_seed(
    num_families: usize,
    num_people_per_family: usize,
    seed: u64,
) -> (ChainState, Bytecodes, Address, Vec<Vec<Address>>) {
    assert!(
        num_people_per_family >= 2,
        "WETH workload needs at least one operator and one user per family"
    );

    let mut rng = StdRng::seed_from_u64(seed);
    let mut families = Vec::with_capacity(num_families);
    let mut operators = Vec::with_capacity(num_families);
    let mut users = Vec::new();

    for _ in 0..num_families {
        let family = (0..num_people_per_family)
            .map(|_| deterministic_address(&mut rng))
            .collect::<Vec<_>>();
        operators.push(family[0]);
        users.extend(family.iter().copied().skip(1));
        families.push(family);
    }

    let weth_address = deterministic_address(&mut rng);
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
