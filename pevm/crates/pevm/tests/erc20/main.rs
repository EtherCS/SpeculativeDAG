//! Each cluster has one ERC20 contract and X families.
//! Each family has Y people.
//! Each person performs Z transfers to random people within the family.

#[path = "../common/mod.rs"]
pub mod common;

#[path = "./mod.rs"]
pub mod erc20;

#[path = "./workload_generation.rs"]
pub mod workload_generation;

use common::test_execute_revm;
use erc20::generate_cluster;
use pevm::chain::PevmEthereum;
use pevm::api;
use pevm::serialization::deserializer;
use pevm::{Bytecodes, ChainState, EvmAccount, InMemoryStorage};
use revm::primitives::{Address, TxEnv, SpecId, BlockEnv};
use std::sync::{Arc, RwLock};
use std::io::Write;

use pevm::{
    Pevm, BuildSuffixHasher,
};

pub use ethers::types::Address as EthAddress;

#[test]
fn erc20_independent() {
    const N: usize = 37123;
    let (mut state, bytecodes, txs) = generate_cluster(N, 1, 1);
    state.insert(Address::ZERO, EvmAccount::default()); // Beneficiary
    test_execute_revm(
        &PevmEthereum::mainnet(),
        InMemoryStorage::new(state, Arc::new(bytecodes), Default::default()),
        txs,
    );
}

#[test]
fn erc20_clusters() {
    const NUM_CLUSTERS: usize = 10;
    const NUM_FAMILIES_PER_CLUSTER: usize = 15;
    const NUM_PEOPLE_PER_FAMILY: usize = 15;
    const NUM_TRANSFERS_PER_PERSON: usize = 15;

    let mut final_state = ChainState::default();
    final_state.insert(Address::ZERO, EvmAccount::default()); // Beneficiary
    let mut final_bytecodes = Bytecodes::default();
    let mut final_txs = Vec::<TxEnv>::new();
    for _ in 0..NUM_CLUSTERS {
        let (state, bytecodes, txs) = generate_cluster(
            NUM_FAMILIES_PER_CLUSTER,
            NUM_PEOPLE_PER_FAMILY,
            NUM_TRANSFERS_PER_PERSON,
        );
        final_state.extend(state);
        final_bytecodes.extend(bytecodes);
        final_txs.extend(txs);
    }
    common::test_execute_revm(
        &PevmEthereum::mainnet(),
        InMemoryStorage::new(final_state, Arc::new(final_bytecodes), Default::default()),
        final_txs,
    )
}

#[test]
fn small_cluster() {
    const N: usize = 1;
    let (mut state, bytecodes, txs) = generate_cluster(N, 2, 2);

    state.insert(Address::ZERO, EvmAccount::default()); // Beneficiary

    let mut state_clone = state.clone();

    println!("state: {state:?}");

    let first_2_txs: Vec<TxEnv> = txs.clone().into_iter().take(2).collect();
    let last_2_txs: Vec<TxEnv> = txs.clone().into_iter().skip(2).take(2).collect();

    //  println!("first_2_txs: {first_2_txs:#?}");

    let storage = InMemoryStorage::new(state, Arc::new(bytecodes.clone()), Default::default());
    let chain = PevmEthereum::mainnet();

    let concurrency_level = std::thread::available_parallelism().unwrap_or(std::num::NonZeroUsize::MIN);

    let result = Pevm::default().execute_revm_parallel(
        &chain,
        &storage,
        SpecId::LATEST,
        BlockEnv::default(),
        first_2_txs,
        concurrency_level,
    );


    let output_file = std::fs::File::create("erc20_small_cluster_output.txt").unwrap();
    
    for changed_state in result.as_ref().unwrap().iter() {
        for (addr, acc) in changed_state.state.iter() {
            match acc {
                Some(account) => {
                    state_clone.insert(*addr, account.clone());
                }
                None => {
                    state_clone.remove(addr);
                }
            }
        }
    }

    let mut writer = std::io::BufWriter::new(output_file);
    // writer.write_all(result_str.as_bytes()).unwrap();
    // writer.flush().unwrap();

    println!("new state: {state_clone:?}");

    let storage = InMemoryStorage::new(state_clone, Arc::new(bytecodes.clone()), Default::default());

    let result = Pevm::default().execute_revm_parallel(
        &chain,
        &storage,
        SpecId::LATEST,
        BlockEnv::default(),
        last_2_txs,
        concurrency_level,
    );

    let result_str = format!("{result:#?}");
    // output the results to a file
    writer.write_all(result_str.as_bytes()).unwrap();
    writer.flush().unwrap();

}


#[tokio::test]
async fn test_workload_generation() -> Result<(), Box<dyn std::error::Error>> {
    workload_generation::generate_workload().await;
    Ok(())
}

#[test]
fn split_test() -> std::io::Result<()> {
    workload_generation::split_file_round_robin("erc_20_workload", 4);
    Ok(())
}
