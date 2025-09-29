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
use revm::primitives::{Address, TxEnv};
use std::sync::Arc;

pub use ethers::types::Address as EthAddress;
