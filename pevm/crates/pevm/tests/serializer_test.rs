use pevm::serialization::serializer;
use pevm::serialization::deserializer;
use pevm::serialization::adapter;
use ethers::types::{
    transaction::eip2718::TypedTransaction,
    transaction::eip1559::Eip1559TransactionRequest,
    transaction::eip2930::{AccessList, AccessListItem},
    TransactionRequest, NameOrAddress, Address, U256, Bytes, Signature, H256, U64,
};

use revm::{
    primitives::{AuthorizationList, BlockEnv, SpecId, TxEnv, ruint::Uint},
    Handler,
};

use alloy_primitives::{TxKind, B256};
use alloy_primitives::Bytes as AlloyBytes;
use alloy_primitives::Address as AlloyAddress;
use alloy_rpc_types_eth::AccessListItem as AlloyAccessListItem;

pub use ethers::signers::{LocalWallet, Signer};
pub use hex::FromHex;
pub use rlp::Rlp;
pub use std::env;
use std::str::FromStr;

// #[test]
// fn test_serializer() {
//     serializer::test();
// }

#[tokio::test]
async fn test_serializer() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = LocalWallet::new(&mut rand::thread_rng()).with_chain_id(1u64);
    let pubkey = wallet.signer().verifying_key(); // 返回 k256::PublicKey
    let pubkey_uncompressed = pubkey.to_encoded_point(false);
    println!("0x{}", hex::encode(pubkey_uncompressed.as_bytes()));
    println!("Address: {:?}", wallet.address());

    // let raw_1559_signed = serializer::build_and_sign_tx(
    //     &wallet,
    //     false, // false = EIP-1559
    //     "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
    //     U256::from(100000000000000000u128), // 0.1 ETH
    //     21000,
    //     Some(1_500_000_000u64), // MaxPriorityFeePerGas / GasPrice
    //     Some(2_000_000_000u64), // MaxFeePerGas / GasPrice
    //     0, // Nonce
    // )
    // .await?;

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

    let txn = Eip1559TransactionRequest {
        from: Some(Address::from_str("0xd8da6bf26964af9d7eed9e03e534151234567890").unwrap_or_default()),
        to: Some(NameOrAddress::Address(Address::from_str("0xd8da6bf26964af9d7eed9e03e53415d37aa96045").unwrap_or_default())),
        gas: Some(21000u64.into()),
        value: Some(U256::from(100000000000000000u128)),
        data: Some(Vec::new().into()),
        nonce: Some(0.into()),
        chain_id: Some(1u64.into()),
        max_priority_fee_per_gas: Some(U256::from(1_500_000_000u64)),
        max_fee_per_gas: Some(U256::from(2_000_000_000u64)),
        access_list: access_list,
    };

    let raw_1559_unsigned = serializer::encode_tx_unsigned(txn)?;

    println!("");
    println!("unsigend EIP-1559 raw tx: {}", raw_1559_unsigned);
    println!("");


    let deserialized_tx = deserializer::decode_hex(&raw_1559_unsigned);

    println!("{:#?}", deserialized_tx);

    // let tx_env = adapter::adapt_transaction(deserialized_tx);

    // println!("{:#?}", tx_env);

    // let (eip1559, caller) = adapter::adapt_tx_env(tx_env);

    // println!("Final Result:");

    // println!("{:#?}", eip1559);
    // println!("{:#?}", caller);

    Ok(())
}


#[tokio::test]
async fn txenv_2_eip1559_2_hex_eip1559_2_txenv() -> Result<(), Box<dyn std::error::Error>> {
    let tx_env: TxEnv = TxEnv {
        caller: AlloyAddress::new(adapter::get_address_slice(&Address::from_str("0xd8da6bf26964af9d7eed9e03e534151234567890").unwrap())),
        gas_limit: 21000,
        gas_price: Uint::from(1_500_000_000u64),
        // gas_price: Some(U256::from(1_500_000_000u64)),
        transact_to: TxKind::Call(AlloyAddress::from_str("0xd8da6bf26964af9d7eed9e03e53415d37aa96045").unwrap()),
        value: Uint::from(100000000000000000u128),
        data: AlloyBytes::default(),
        nonce: Some(0),
        chain_id: Some(1),
        ..Default::default()
    };

    println!("TxEnv: {:#?}", tx_env);

    let wallet = LocalWallet::new(&mut rand::thread_rng()).with_chain_id(1u64);
    let pubkey = wallet.signer().verifying_key(); // 返回 k256::PublicKey
    let pubkey_uncompressed = pubkey.to_encoded_point(false);
    // println!("0x{}", hex::encode(pubkey_uncompressed.as_bytes()));
    // println!("Address: {:?}", wallet.address());

    let (txn, caller) = adapter::adapt_tx_env(tx_env);

    let raw_1559_unsigned = serializer::encode_tx_unsigned(txn)?;

    println!("");
    println!("unsigend EIP-1559 raw tx: {}", raw_1559_unsigned);
    println!("");


    let deserialized_tx = deserializer::decode_hex(&raw_1559_unsigned);

    // println!("{:#?}", deserialized_tx);

    // let tx_env = adapter::adapt_transaction(deserialized_tx);

    // println!("{:#?}", tx_env);

    Ok(())
}


#[tokio::test]
async fn write_to_file_test() -> Result<(), Box<dyn std::error::Error>> {
    let file_path = "test_output.txt";
    let tx_env: TxEnv = TxEnv {
        caller: AlloyAddress::new(adapter::get_address_slice(&Address::from_str("0xd8da6bf26964af9d7eed9e03e534151234567890").unwrap())),
        gas_limit: 21000,
        gas_price: Uint::from(1_500_000_000u64),
        // gas_price: Some(U256::from(1_500_000_000u64)),
        transact_to: TxKind::Call(AlloyAddress::from_str("0xd8da6bf26964af9d7eed9e03e53415d37aa96045").unwrap()),
        value: Uint::from(100000000000000000u128),
        data: AlloyBytes::default(),
        nonce: Some(0),
        chain_id: Some(1),
        ..Default::default()
    };
    serializer::write_to_file(tx_env, file_path).await?;
    let batch = deserializer::read_from_file(file_path)?;
    println!("Read from file: {:#?}", batch);

    let result = deserializer::decode_batch_hex(batch);
    println!("Decoded Results: {:#?}", result);

    Ok(())
}