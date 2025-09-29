use ethers::types::{
    transaction::eip2718::TypedTransaction,
    transaction::eip1559::Eip1559TransactionRequest,
    transaction::eip2930::{AccessList, AccessListItem},
    TransactionRequest, NameOrAddress, Address, U256, Bytes, Signature, H256, U64,
};
use ethers::signers::{LocalWallet, Signer};
use rlp::Encodable;
use std::str::FromStr;
use rand;
use super::adapter;
use std::fs::OpenOptions;
use std::fs::File;
use std::io::Write;

use revm::{
    primitives::{AuthorizationList, BlockEnv, SpecId, TxEnv, ruint::Uint},
    Handler,
};

pub fn encode_batch_to_hex(
    tx_envs: Vec<TxEnv>,
) -> Vec<(String, Address)> {
    let mut codes_and_callers = Vec::new();
    for tx_env in tx_envs {
        let (tx, caller) = adapter::adapt_tx_env(tx_env);
        let encoded_tx = encode_tx_unsigned(tx).unwrap_or_default();
        codes_and_callers.push((encoded_tx, caller));
    }
    codes_and_callers
}

pub async fn write_to_file(
    tx_env: TxEnv,
    file_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, caller) = adapter::adapt_tx_env(tx_env);
    // println!("caller: {:?}", caller);
    let encoded_tx = encode_tx_unsigned(tx).unwrap_or_default();
    let hex_and_caller = format!("{} {:?}", encoded_tx, caller);

    // Open file in append mode (create if not exists)
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)?;
    // Write the result into the file
    writeln!(file, "{}", hex_and_caller)?;
    Ok(())
}


pub fn encode_tx_unsigned<T>(
    tx: T
) -> Result<String, Box<dyn std::error::Error>> 
where 
    T: Into<TypedTransaction>,
{
    let typed_tx: TypedTransaction = tx.into();
    let raw_bytes = typed_tx.rlp();
    let raw_tx_hex = format!("0x{}", hex::encode(raw_bytes));
    // println!("unsigned raw tx: {}", raw_tx_hex);
    Ok(raw_tx_hex)
}

pub async fn encode_tx_signed<T>(
    tx: T,
    wallet: &LocalWallet,
) -> Result<String, Box<dyn std::error::Error>> 
where 
    T: Into<TypedTransaction>,
{
    let typed_tx: TypedTransaction = tx.into();
    let sig: Signature = wallet.sign_transaction(&typed_tx).await?;
    let signed_tx = typed_tx.clone().rlp_signed(&sig);
    let raw_tx_hex = format!("0x{}", hex::encode(signed_tx));
    // println!("signed raw tx: {}", raw_tx_hex);
    Ok(raw_tx_hex)
}

/// Generate and sign a transaction, either EIP-1559 or Legacy
pub async fn build_and_sign_tx(
    wallet: &LocalWallet,
    use_legacy: bool,
    to_addr: &str,
    value: U256,
    gas_limit: u64,
    gas_price_or_priority_fee: Option<u64>,
    max_fee_per_gas: Option<u64>,
    nonce: u64,
) -> Result<String, Box<dyn std::error::Error>> {
    let to = Address::from_str(to_addr)?;

    let typed_tx: TypedTransaction = if use_legacy {
        // Legacy 交易
        TransactionRequest {
            from: None,
            to: Some(NameOrAddress::Address(to)),
            gas: Some(gas_limit.into()),
            gas_price: Some(gas_price_or_priority_fee.map(U256::from).unwrap_or_default()),
            value: Some(value),
            data: Some(Vec::new().into()),
            nonce: Some(nonce.into()),
            ..Default::default()
        }
        .into()
    } else {
        // EIP-1559 交易
        let mut access_list_items = Vec::<AccessListItem>::new();
        access_list_items.push(AccessListItem {
            address: Address::from_str("0x0000000000000000000000000000000012345678")?,
            storage_keys: vec![H256::from_low_u64_be(0x1), H256::from_low_u64_be(0x2), H256::from_low_u64_be(0x3)],
        });
        access_list_items.push(AccessListItem {
            address: Address::from_str("0x0000000000000000000000000000000012340000")?,
            storage_keys: vec![H256::from_low_u64_be(0x4), H256::from_low_u64_be(0x5), H256::from_low_u64_be(0x6)],
        });
        let access_list = AccessList::from(access_list_items);

        Eip1559TransactionRequest {
            from: None,
            to: Some(NameOrAddress::Address(to)),
            gas: Some(gas_limit.into()),
            value: Some(value),
            data: Some(Vec::new().into()),
            nonce: Some(nonce.into()),
            chain_id: Some(1u64.into()),
            max_priority_fee_per_gas: gas_price_or_priority_fee.map(U256::from),
            max_fee_per_gas: max_fee_per_gas.map(U256::from),
            access_list: access_list,
        }
        .into()
    };

    let raw_bytes_us = typed_tx.rlp();
    let raw_tx_hex_us = format!("0x{}", hex::encode(raw_bytes_us));
    println!("unsigned raw tx: {}", raw_tx_hex_us);

    let sig: Signature = wallet.sign_transaction(&typed_tx).await?;
    let signed_tx = typed_tx.clone().rlp_signed(&sig);
    let raw_tx_hex = format!("0x{}", hex::encode(signed_tx));
    Ok(raw_tx_hex)
}

pub async fn build_tx_unsigned(
    use_legacy: bool,
    to_addr: &str,
    value: U256,
    gas_limit: u64,
    gas_price_or_priority_fee: Option<u64>,
    max_fee_per_gas: Option<u64>,
    nonce: u64,
) -> Result<String, Box<dyn std::error::Error>> {
    let to = Address::from_str(to_addr)?;

    let typed_tx: TypedTransaction = if use_legacy {
        // Legacy
        TransactionRequest {
            from: None,
            to: Some(NameOrAddress::Address(to)),
            gas: Some(gas_limit.into()),
            gas_price: gas_price_or_priority_fee.map(U256::from),
            value: Some(value),
            data: Some(Vec::new().into()),
            nonce: Some(nonce.into()),
            ..Default::default()
        }
        .into()
    } else {
        // EIP-1559
        Eip1559TransactionRequest {
            from: None,
            to: Some(NameOrAddress::Address(to)),
            gas: Some(gas_limit.into()),
            value: Some(value),
            data: Some(Vec::new().into()),
            nonce: Some(nonce.into()),
            chain_id: Some(1u64.into()),
            max_priority_fee_per_gas: gas_price_or_priority_fee.map(U256::from),
            max_fee_per_gas: max_fee_per_gas.map(U256::from),
            access_list: AccessList::default(),
        }
        .into()
    };

    let raw_bytes = typed_tx.rlp();
    let raw_tx_hex = format!("0x{}", hex::encode(raw_bytes));

    Ok(raw_tx_hex)
}


pub async fn test() -> Result<(), Box<dyn std::error::Error>> {
    // A wallet with random private key
    let wallet = LocalWallet::new(&mut rand::thread_rng()).with_chain_id(1u64);

    // Test: Generate and sign EIP-1559 transaction
    let raw_1559 = build_tx_unsigned(
        false, // false = EIP-1559
        "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
        U256::from(100000000000000000u128), // 0.1 ETH
        21000,
        Some(1_500_000_000u64), // MaxPriorityFeePerGas / GasPrice
        Some(2_000_000_000u64), // MaxFeePerGas / GasPrice
        0, // Nonce
    )
    .await?;
    println!("unsigned EIP-1559 raw tx: {}", raw_1559);

    // Test: Generate an unsigned EIP-1559 transaction
    let raw_1559_signed = build_and_sign_tx(
        &wallet,
        false, // false = EIP-1559
        "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
        U256::from(100000000000000000u128), // 0.1 ETH
        21000,
        Some(1_500_000_000u64), // MaxPriorityFeePerGas / GasPrice
        Some(2_000_000_000u64), // MaxFeePerGas / GasPrice
        0, // Nonce
    )
    .await?;
    println!("");
    println!("sigend EIP-1559 raw tx: {}", raw_1559_signed);
    println!("");

    // Test: Generate and sign Legacy transaction
    let raw_legacy = build_and_sign_tx(
        &wallet,
        true, // true = Legacy
        "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
        U256::from(200000000000000000u128), // 0.2 ETH
        21000,
        Some(20_000_000_000u64), // GasPrice
        None,                    // MaxFeePerGas (Ignored in legacy)
        1, // Nonce
    )
    .await?;
    println!("Legacy raw tx: {}", raw_legacy);

    Ok(())
}
