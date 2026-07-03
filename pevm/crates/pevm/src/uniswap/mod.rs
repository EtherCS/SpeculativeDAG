//! Uniswap V3 workload generation helpers.

pub mod contract;

use crate::erc20::contract::ERC20Token;
use crate::{Bytecodes, ChainState, EvmAccount};
use contract::{SingleSwap, SwapRouter, UniswapV3Factory, UniswapV3Pool, WETH9};
use revm::primitives::{fixed_bytes, uint, Address, Bytes, B256, U256};

pub fn generate_state_and_byte_code(
    num_families: usize,
    num_people_per_family: usize,
) -> (ChainState, Bytecodes, Address, Vec<Vec<Address>>) {
    let families: Vec<Vec<Address>> = (0..num_families)
        .map(|_| {
            (0..num_people_per_family)
                .map(|_| Address::new(rand::random()))
                .collect()
        })
        .collect();
    let people_addresses: Vec<Address> = families.iter().flatten().copied().collect();

    let (dai_address, usdc_address) = {
        let x = Address::new(rand::random());
        let y = Address::new(rand::random());
        (std::cmp::min(x, y), std::cmp::max(x, y))
    };

    let pool_init_code_hash = B256::new(rand::random());
    let swap_router_address = Address::new(rand::random());
    let single_swap_address = Address::new(rand::random());
    let weth9_address = Address::new(rand::random());
    let owner = Address::new(rand::random());
    let factory_address = Address::new(rand::random());
    let nonfungible_position_manager_address = Address::new(rand::random());
    let pool_address = UniswapV3Pool::new(dai_address, usdc_address, factory_address)
        .get_address(factory_address, pool_init_code_hash);

    let weth9_account = WETH9::new().build();
    let dai_account = ERC20Token::new("DAI", "DAI", 18, 222_222_000_000_000_000_000_000u128)
        .add_balances(&[pool_address], uint!(111_111_000_000_000_000_000_000_U256))
        .add_balances(&people_addresses, uint!(1_000_000_000_000_000_000_U256))
        .add_allowances(
            &people_addresses,
            single_swap_address,
            uint!(1_000_000_000_000_000_000_U256),
        )
        .build();
    let usdc_account = ERC20Token::new("USDC", "USDC", 18, 222_222_000_000_000_000_000_000u128)
        .add_balances(&[pool_address], uint!(111_111_000_000_000_000_000_000_U256))
        .add_balances(&people_addresses, uint!(1_000_000_000_000_000_000_U256))
        .add_allowances(
            &people_addresses,
            single_swap_address,
            uint!(1_000_000_000_000_000_000_U256),
        )
        .build();
    let factory_account = UniswapV3Factory::new(owner)
        .add_pool(dai_address, usdc_address, pool_address)
        .build(factory_address);
    let pool_account = UniswapV3Pool::new(dai_address, usdc_address, factory_address)
        .add_position(
            nonfungible_position_manager_address,
            -600000,
            600000,
            [
                uint!(0x00000000000000000000000000000000000000000000178756e190b388651605_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
            ],
        )
        .add_tick(
            -600000,
            [
                uint!(0x000000000000178756e190b388651605000000000000178756e190b388651605_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
                uint!(0x0100000001000000000000000000000000000000000000000000000000000000_U256),
            ],
        )
        .add_tick(
            600000,
            [
                uint!(0xffffffffffffe878a91e6f4c779ae9fb000000000000178756e190b388651605_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
                uint!(0x0000000000000000000000000000000000000000000000000000000000000000_U256),
                uint!(0x0100000000000000000000000000000000000000000000000000000000000000_U256),
            ],
        )
        .build(pool_address);
    let swap_router_account =
        SwapRouter::new(weth9_address, factory_address, pool_init_code_hash).build();
    let single_swap_account =
        SingleSwap::new(swap_router_address, dai_address, usdc_address).build();

    let mut state = ChainState::from_iter([
        (weth9_address, weth9_account),
        (dai_address, dai_account),
        (usdc_address, usdc_account),
        (factory_address, factory_account),
        (pool_address, pool_account),
        (swap_router_address, swap_router_account),
        (single_swap_address, single_swap_account),
    ]);

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
        if let Some(code) = account.code.take() {
            bytecodes.insert(account.code_hash.unwrap(), code);
        }
    }

    (state, bytecodes, single_swap_address, families)
}

pub fn sell_token0(amount: U256) -> Bytes {
    Bytes::from([&fixed_bytes!("c92b0891")[..], &B256::from(amount)[..]].concat())
}

pub fn sell_token1(amount: U256) -> Bytes {
    Bytes::from([&fixed_bytes!("6b055260")[..], &B256::from(amount)[..]].concat())
}

pub fn buy_token0(amount_out: U256, amount_in_max: U256) -> Bytes {
    Bytes::from(
        [
            &fixed_bytes!("8dc33f82")[..],
            &B256::from(amount_out)[..],
            &B256::from(amount_in_max)[..],
        ]
        .concat(),
    )
}

pub fn buy_token1(amount_out: U256, amount_in_max: U256) -> Bytes {
    Bytes::from(
        [
            &fixed_bytes!("b2db18a2")[..],
            &B256::from(amount_out)[..],
            &B256::from(amount_in_max)[..],
        ]
        .concat(),
    )
}
