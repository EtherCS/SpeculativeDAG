use crate::common::storage::{from_address, from_indices, from_short_string, StorageBuilder};
use crate::EvmAccount;
use revm::primitives::{fixed_bytes, hex::FromHex, Address, Bytecode, Bytes, B256, U256};

const WETH9_HEX: &str = include_str!("./assets/WETH9.hex");

#[derive(Debug, Clone)]
pub struct WETH9 {
    name: String,
    symbol: String,
    decimals: u8,
    balances: Vec<(Address, U256)>,
    allowances: Vec<(Address, Address, U256)>,
}

impl Default for WETH9 {
    fn default() -> Self {
        Self::new("Wrapped Ether", "WETH", 18)
    }
}

impl WETH9 {
    pub fn new(name: &str, symbol: &str, decimals: u8) -> Self {
        Self {
            name: name.to_string(),
            symbol: symbol.to_string(),
            decimals,
            balances: Vec::new(),
            allowances: Vec::new(),
        }
    }

    pub fn add_balances(mut self, holders: &[Address], amount: U256) -> Self {
        self.balances
            .extend(holders.iter().copied().map(|holder| (holder, amount)));
        self
    }

    pub fn add_allowance(mut self, owner: Address, spender: Address, amount: U256) -> Self {
        self.allowances.push((owner, spender, amount));
        self
    }

    pub fn add_allowances_to_family_operators(
        mut self,
        families: &[Vec<Address>],
        amount: U256,
    ) -> Self {
        for family in families {
            if family.len() < 2 {
                continue;
            }
            let operator = family[0];
            for owner in family.iter().copied().skip(1) {
                self.allowances.push((owner, operator, amount));
            }
        }
        self
    }

    pub fn build(&self) -> EvmAccount {
        let bytecode = Bytecode::new_raw(Bytes::from_hex(WETH9_HEX.trim()).unwrap());
        let mut store = StorageBuilder::new();
        store.set(0, from_short_string(&self.name));
        store.set(1, from_short_string(&self.symbol));
        store.set(2, self.decimals);
        store.set(3, 0);
        store.set(4, 0);

        for (holder, amount) in &self.balances {
            store.set(from_indices(3, &[from_address(*holder)]), *amount);
        }

        for (owner, spender, amount) in &self.allowances {
            store.set(
                from_indices(4, &[from_address(*owner), from_address(*spender)]),
                *amount,
            );
        }

        EvmAccount {
            balance: U256::ZERO,
            nonce: 1u64,
            code_hash: Some(bytecode.hash_slow()),
            code: Some(bytecode.into()),
            storage: store.build(),
        }
    }

    pub fn deposit() -> Bytes {
        Bytes::from(fixed_bytes!("d0e30db0").to_vec())
    }

    pub fn withdraw(amount: U256) -> Bytes {
        Bytes::from([&fixed_bytes!("2e1a7d4d")[..], &B256::from(amount)[..]].concat())
    }

    pub fn transfer(recipient: Address, amount: U256) -> Bytes {
        Bytes::from(
            [
                &fixed_bytes!("a9059cbb")[..],
                &B256::left_padding_from(recipient.as_slice())[..],
                &B256::from(amount)[..],
            ]
            .concat(),
        )
    }

    pub fn approve(spender: Address, amount: U256) -> Bytes {
        Bytes::from(
            [
                &fixed_bytes!("095ea7b3")[..],
                &B256::left_padding_from(spender.as_slice())[..],
                &B256::from(amount)[..],
            ]
            .concat(),
        )
    }

    pub fn transfer_from(owner: Address, recipient: Address, amount: U256) -> Bytes {
        Bytes::from(
            [
                &fixed_bytes!("23b872dd")[..],
                &B256::left_padding_from(owner.as_slice())[..],
                &B256::left_padding_from(recipient.as_slice())[..],
                &B256::from(amount)[..],
            ]
            .concat(),
        )
    }
}
