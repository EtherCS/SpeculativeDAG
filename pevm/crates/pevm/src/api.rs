#![allow(unused)]

// Provide Pevm API for transaction scheduling and execution, used by Mysticeti.
use std::{fmt, fs};
use std::error::Error;
// #[cfg(feature = "with-tokio")]
use tokio::time::{sleep, Duration, Instant};
// #[cfg(feature = "with-tokio")]
use tokio::sync::{Mutex};
use tokio::sync::mpsc;

use std::{
    collections::{VecDeque, HashMap, HashSet},
    num::NonZeroUsize,
    thread,
    sync::{Arc, Mutex as StdMutex},
};

use crossbeam_deque::{Injector, Stealer, Worker};
use crossbeam_utils::Backoff;
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;

use crate::{
    Pevm,
    erc20::contract::ERC20Token,
    vm::PevmTxExecutionResult,
};

use revm::primitives::{alloy_primitives::U160, BlockEnv, SpecId, TxEnv, U256, TransactTo};

use ethers::types::{
    Address, 
};

use alloy_primitives::Address as AlloyAddress;

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter};
use std::path::Path;

use serde::{Serialize, Deserialize};

use super::serialization::{deserializer, serializer};
use crate::{Bytecodes, ChainState, EvmAccount, InMemoryStorage, chain::PevmEthereum};
use super::erc20;

use serde_json;

fn save(storage: &InMemoryStorage, path: &str) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer(writer, storage)?;
    Ok(())
}

fn load(path: &str) -> anyhow::Result<InMemoryStorage> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let storage = serde_json::from_reader(reader)?;
    Ok(storage)
}

pub fn load_in_memory_storage(workload_type: &WorkloadType) -> InMemoryStorage {
    match workload_type {
        WorkloadType::ERC20(num_clusters, num_families_per_cluster, num_people_per_family) => {
            let path = format!("/home/ubuntu/congestion_control/pevm/crates/pevm/storage_{}_{}_{}.json", num_clusters, num_families_per_cluster, num_people_per_family);
            println!("in memory storage file path: {}", path);
            load(&path).unwrap()
        }
    }
}

pub fn load_account_addresses(workload_type: &WorkloadType) -> Vec<(AlloyAddress, Vec<Vec<AlloyAddress>>)> {
    match workload_type {
        WorkloadType::ERC20(num_clusters, num_families_per_cluster, num_people_per_family) => {
            let path = format!("/home/ubuntu/congestion_control/pevm/crates/pevm/account_addresses_{}_{}_{}.bin", num_clusters, num_families_per_cluster, num_people_per_family);
            println!("account addresses file path: {}", path);
            load_addresses(&path).unwrap()
        }
    }
}

type Addresses = Vec<(AlloyAddress, Vec<Vec<AlloyAddress>>)>;

fn save_addresses(path: &str, data: &Addresses) -> anyhow::Result<()> {
    let encoded = bincode::serialize(data)?;        // binary encoding
    fs::write(path, encoded)?;
    Ok(())
}

fn load_addresses(path: &str) -> anyhow::Result<Addresses> {
    let bytes = fs::read(path)?;
    let decoded: Addresses = bincode::deserialize(&bytes)?;
    Ok(decoded)
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionWithHint {
    pub raw_hex: String,
    pub caller: Address,
    pub hint: String, // [TODO] The type 'String' is a placeholder for now
}

/// A list of Error types that can be returned by the Pevm API.
#[derive(Debug, Clone)]
pub enum APIError {
    NoScheduledTransactions,
    NoWorkloadFile,
}

impl fmt::Display for APIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            APIError::NoScheduledTransactions => write!(f, "No scheduled transactions available"),
            APIError::NoWorkloadFile => write!(f, "No workload file available"),
        }
    }
}

impl Error for APIError {}

#[derive(Debug)]
pub struct PevmAPI {
    pub pevm: Pevm,
    pub txns_queue: Mutex<VecDeque<TransactionWithHint>>,
    pub scheduled_txns: Mutex<VecDeque<TransactionWithHint>>,
    pub addresses: Vec<(AlloyAddress, Vec<Vec<AlloyAddress>>)>, 
    pub workload_type: WorkloadType,
    pub in_memory_storage: InMemoryStorage,
}

impl PevmAPI {
    pub fn new(workload_type: WorkloadType) -> Self {
        let addresses = load_account_addresses(&workload_type);
        let in_memory_storage = load_in_memory_storage(&workload_type);
        Self {
            pevm: Pevm::default(),
            txns_queue: Mutex::new(VecDeque::new()),
            scheduled_txns: Mutex::new(VecDeque::new()),
            workload_type,
            addresses,
            in_memory_storage,
        }
    }

    pub async fn num_pending_txns(&self) -> usize {
        let queue = self.txns_queue.lock().await;
        let scheduled = self.scheduled_txns.lock().await;
        queue.len() + scheduled.len()
    }

    pub async fn add_transactions(&mut self, transactions: Vec<TransactionWithHint>) {
        tracing::info!("Waiting queue lock");
        let mut queue = self.txns_queue.lock().await;
        for txn in transactions {
            tracing::info!("Adding transaction: {:?}", txn);
            queue.push_back(txn);
        }
    }
    

    pub async fn fetch_one_scheduled_txn(&mut self) -> Result<TransactionWithHint, APIError> {
        let mut scheduled_queue = self.scheduled_txns.lock().await;
        if scheduled_queue.is_empty() {
            tracing::info!("No scheduled transactions");
            return Err(APIError::NoScheduledTransactions);
        }

        scheduled_queue
            .pop_front()
            .ok_or(APIError::NoScheduledTransactions)
    }

    pub fn get_erc20_state_and_bytecode(num_clusters: usize, num_families_per_cluster: usize, num_people_per_family: usize) -> (InMemoryStorage, Vec<(AlloyAddress, Vec<Vec<AlloyAddress>>)>) {
        let mut addresses = Vec::new();
        let mut final_state = ChainState::default();
        let mut final_bytecodes = Bytecodes::default();
        final_state.insert(AlloyAddress::ZERO, EvmAccount::default()); // Beneficiary
        for _ in 0..num_clusters {
            let (state, bytecodes, gld_address, families) = erc20::generate_state_and_byte_code(num_families_per_cluster, num_people_per_family);
            final_state.extend(state);
            final_bytecodes.extend(bytecodes);
            addresses.push((gld_address, families));
        }
        let in_memory_storage = InMemoryStorage::new(final_state, Arc::new(final_bytecodes), Default::default());
        (in_memory_storage, addresses)
    }

}


#[derive(Debug, Clone, Default)]
pub enum ExecutionMode {
    #[default] Sequential,
    Parallel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkloadType {
    ERC20(usize, usize, usize), // NUM_CLUSTERS, NUM_FAMILY_PER_CLUSTER, NUM_PEOPLE_PER_FAMILY
}

impl Default for WorkloadType {
    fn default() -> Self {
        WorkloadType::ERC20(1, 1, 1) // or your preferred defaults
    }
}

pub struct PevmExecutor {
    pub execution_mode: ExecutionMode,
    pub storage: InMemoryStorage,
    pub chain: PevmEthereum,
}

impl PevmExecutor {
    pub fn new(execution_mode: ExecutionMode, workload_type: WorkloadType) -> Self {
        Self {
            execution_mode,
            storage: load_in_memory_storage(&workload_type),
            chain: PevmEthereum::mainnet(),
        }
    }

    fn load_from_json(path: &str) -> InMemoryStorage {
        load(path).expect("Failed to load InMemoryStorage from JSON")
    }

    fn update_storage(&mut self, results: Vec<PevmTxExecutionResult>) {
        let mut state = self.storage.accounts_clone();
        for changed_state in results.iter() {
            for (addr, acc) in changed_state.state.iter() {
                match acc {
                    Some(account) => {
                        state.insert(*addr, account.clone());
                    }
                    None => {
                        state.remove(addr);
                    }
                }
            }
        }
        self.storage.update_accounts(state);
    }

    pub fn execute(&mut self, txs: Vec<(String, Address)>) {
        let mut txs = deserializer::decode_batch_hex(txs);

        match self.execution_mode {
            ExecutionMode::Sequential => {
                tracing::info!("Executed transactions sequentially");
                let result = crate::execute_revm_sequential(
                    &self.chain,
                    &self.storage,
                    SpecId::LATEST,
                    BlockEnv::default(),
                    txs,
                );
                self.update_storage(result.unwrap());
            }
            ExecutionMode::Parallel => {
                let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
                tracing::info!("Starting Executing {} transactions in parallel with {} threads", &txs.len(), concurrency_level);

                let result = Pevm::default().execute_revm_parallel(
                    &self.chain,
                    &self.storage,
                    SpecId::LATEST,
                    BlockEnv::default(),
                    txs,
                    concurrency_level,
                );
                tracing::info!("Executed transactions in parallel with {} threads", concurrency_level);

                match &result {
                    Ok(res) => tracing::info!("Execution successful with {} results", res.len()),
                    Err(e) => tracing::error!("Execution failed: {:?}", e),
                }

                self.update_storage(result.unwrap());
            }
        }
    }
}



pub struct PevmTransactionGenerator {
    pub workload_type: WorkloadType,
    pub clusters: Vec::<(AlloyAddress, Vec<Vec<AlloyAddress>>)>,
    pub nonce: u64,
    pub replica_id: u64,
    pub replica_num: u64,
    pub pevm_txn_sender: mpsc::Sender<Vec<(String, Address)>>,
    pub insufficient_txn_signal_receiver: mpsc::Receiver<usize>,
    pub nonce_map: HashMap<AlloyAddress, u64>,
    pub high_contention_interval: Option<u64>,
}

impl PevmTransactionGenerator {
    pub fn new(workload_type: WorkloadType, replica_id: u64, replica_num: u64, pevm_txn_sender: mpsc::Sender<Vec<(String, Address)>>, insufficient_txn_signal_receiver: mpsc::Receiver<usize>) -> Self {
        let clusters = load_account_addresses(&workload_type);
        let mut nonce_map = HashMap::new();
        for (gld_address, families) in &clusters {
            for family in families {
                for member in family {
                    nonce_map.insert(AlloyAddress::from(*member), 0u64);
                }
            }
        }

        Self {
            workload_type,
            clusters,
            nonce: 0,
            replica_id,
            replica_num,
            pevm_txn_sender,
            insufficient_txn_signal_receiver,
            nonce_map: nonce_map,
            high_contention_interval: Some(1), // 1 out of 10 batches of transactions will be contended
        }
    }

    pub async fn run(&mut self) {
        const MAX_PENDING_TRANSACTION_NUM:usize = 1000;
        const INITIAL_BATCH:usize = 100;
        let mut new_transactions = Vec::new();
        tracing::info!("Start Running PEVM");
        loop {
            let batch = self.generate_transactions();
            new_transactions.extend(batch);
            if new_transactions.len() >= MAX_PENDING_TRANSACTION_NUM + INITIAL_BATCH {
                let initial_batch_to_schedule = new_transactions.drain(..INITIAL_BATCH).collect();
                self.pevm_txn_sender.send(initial_batch_to_schedule).await;
                break;
            }
        }

        loop{
            let txn_needed = self.insufficient_txn_signal_receiver.recv().await.unwrap();
            tracing::info!("txn_needed = {}", txn_needed);
            let batch_to_schedule: Vec<(String, Address)> = new_transactions.drain(..txn_needed).collect();
            self.pevm_txn_sender.send(batch_to_schedule).await;
            loop{
                let batch = self.generate_transactions();
                new_transactions.extend(batch);
                if new_transactions.len() >= MAX_PENDING_TRANSACTION_NUM {
                    break;
                }
            }
        }
    }

    pub fn generate_transactions(&mut self) -> Vec<(String, Address)> {
        match self.workload_type {
            WorkloadType::ERC20(_, _, _) => {
                match self.high_contention_interval {
                    Some(_) => self.generate_contended_erc20_transactions(),
                    None => self.generate_parallelizable_erc20_transactions(),
                }
            },
        }
    }

    pub fn generate_parallelizable_erc20_transactions(&mut self) -> Vec<(String, Address)> {
        const GAS_LIMIT: u64 = 35_000;
        let mut transactions = Vec::new();

        let num_people_per_family = match self.workload_type {
            WorkloadType::ERC20(_, _, num_people_per_family) => num_people_per_family,
        };

        for counter in 0..num_people_per_family {
            if counter % self.replica_num as usize != self.replica_id as usize {
                continue; // Each member sends transaction once every 4 iterations
            }
            for (gld_address, families) in &self.clusters {
                for family in families {
                    let member = family[counter];
                    let recipient = family[(rand::random::<usize>()) % (family.len())];
                    let calldata = ERC20Token::transfer(recipient, U256::from(rand::random::<u8>()));
                    transactions.push(TxEnv {
                        caller: member,
                        gas_limit: GAS_LIMIT,
                        gas_price: U256::from(0xb2d05e07u64),
                        transact_to: TransactTo::Call(*gld_address),
                        data: calldata,
                        nonce: Some(self.nonce_map[&member]),
                        chain_id: Some(1),
                        ..TxEnv::default()
                    });
                    self.nonce_map.entry(member).and_modify(|n| *n += 1).or_insert(1);
                }
            }
        }

        let hex_codes = serializer::encode_batch_to_hex(transactions);
        
        hex_codes
    }

    fn generate_contended_erc20_transactions(&mut self) -> Vec<(String, Address)> {
        let random_value = rand::random::<u64>() % self.high_contention_interval.unwrap(); 
        if random_value != 0 {
            println!("Generating parallelizable transactions");
            return self.generate_parallelizable_erc20_transactions();
        }
        println!("Generating contended transactions");
        const GAS_LIMIT: u64 = 35_000;
        let mut transactions = Vec::with_capacity(50);

        let three_random_clusters = {
            let mut indices = Vec::new();
            let mut rng = rand::thread_rng();
            while indices.len() < 2 {
                let idx = rand::random::<usize>() % self.clusters.len();
                if !indices.contains(&idx) {
                    indices.push(idx);
                }
            }
            indices
        };

        let three_random_families: Vec<usize> = three_random_clusters.iter().map(|&cluster_idx| {
            let families = &self.clusters[cluster_idx].1;
            rand::random::<usize>() % families.len()
        }).collect();

        loop {
            for (index, cluster_id) in three_random_clusters.iter().enumerate() {
                let (gld_address, families) = &self.clusters[*cluster_id];
                let family = &families[three_random_families[index]];
                let mut counter = 0;
                for member in family {
                    if counter % self.replica_num != self.replica_id {
                        counter += 1;
                        continue; // Each member sends transaction once every 4 iterations
                    }
                    let recipient = family[(rand::random::<usize>()) % (family.len())];
                    let calldata = ERC20Token::transfer(recipient, U256::from(rand::random::<u8>()));
                    transactions.push(TxEnv {
                        caller: *member,
                        gas_limit: GAS_LIMIT,
                        gas_price: U256::from(0xb2d05e07u64),
                        transact_to: TransactTo::Call(*gld_address),
                        data: calldata,
                        nonce: Some(self.nonce_map[member]),
                        chain_id: Some(1),
                        ..TxEnv::default()
                    });
                    counter += 1;
                    self.nonce_map.entry(*member).and_modify(|n| *n += 1).or_insert(1);
                    if transactions.len() >= 16 {
                        let hex_codes = serializer::encode_batch_to_hex(transactions);
                        return hex_codes
                    }
                }
            }
        }
        Vec::new()
    }



}


pub struct PevmScheduler {
    scheduled_txns: Arc<Mutex<Vec<TransactionWithHint>>>,
    pevm_txn_receiver: Mutex<mpsc::Receiver<Vec<(String, Address)>>>,
}

impl PevmScheduler {
    pub fn new(
        pevm_txn_receiver: mpsc::Receiver<Vec<(String, Address)>>,
    ) -> Self {
        Self {
            scheduled_txns: Arc::new(Mutex::new(Vec::new())),
            pevm_txn_receiver: Mutex::new(pevm_txn_receiver),
        }
    }

    pub async fn run(self: Arc<Self>) {
        tracing::info!("starting running PevmScheduler");
        // Take the receiver exactly once
        let mut rx = self.pevm_txn_receiver
            .lock().await;

        while let Some(batch) = rx.recv().await {
            // tracing::info!("scheduling {} txns", batch.len());
            self.schedule(batch).await;
        }
    }

    pub async fn schedule(&self, batch: Vec<(String, Address)>) {
        let batch: Vec<TransactionWithHint> = batch
            .into_iter()
            .map(|(raw_hex, caller)| TransactionWithHint {
                raw_hex,
                caller,
                hint: String::new(), // or some default value
            }).collect();
        let mut lock = self.scheduled_txns.lock().await;
        lock.extend(batch);
    }

    pub fn schedule_a_parallelizable_batch(txns: &mut Vec<TransactionWithHint>) -> Vec<Option<TransactionWithHint>> {
        let concurrency_level : usize = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN).get();
        let TXNS_PER_BATCH: usize = concurrency_level * 4;

        let mut account_last_position = HashMap::new();
        let mut next_available_position = 0;
        let mut scheduled: Vec<Option<TransactionWithHint>> = vec![None; TXNS_PER_BATCH];
        let mut scheduled_txn_indices = Vec::new();
        let mut available_slots = TXNS_PER_BATCH;

        for (index, txn) in txns.iter().enumerate() {
            let tx_env = deserializer::decode_one_hex(txn.raw_hex.clone(), txn.caller);
            let mut accessed_accounts = Vec::new();
            match tx_env {
                Some(tx_env) => {
                    match tx_env.transact_to {
                        TransactTo::Call(to_addr) => {
                            accessed_accounts.push(tx_env.caller);
                            accessed_accounts.push(to_addr);
                        }
                        TransactTo::Create => {
                            accessed_accounts.push(tx_env.caller);
                        }
                    }

                    let mut never_accessed = true;
                    let mut position_found = false;
                    let mut final_position = 0;
                    for account in accessed_accounts.clone() {
                        if let Some(&last_pos) = account_last_position.get(&account) {
                            // println!("Account {:?} was last accessed at position {}", account, last_pos);
                            never_accessed = false;
                            let mut new_position : usize = last_pos + concurrency_level;
                            while new_position < TXNS_PER_BATCH {
                                // println!("Checking position {} for transaction {}", new_position, index);
                                if scheduled[new_position].is_none() {
                                    // println!("Found position {} for transaction {}", new_position, index);
                                    position_found = true;
                                    final_position = final_position.max(new_position);
                                    break;
                                }
                                new_position += concurrency_level;
                            }
                        }
                    }

                    if never_accessed {
                        position_found = true;
                        final_position = next_available_position;
                    }

                    if position_found {
                        // println!("Scheduling transaction {} at position {}", index, final_position);
                        available_slots -= 1;
                        scheduled_txn_indices.push(index);
                        scheduled[final_position] = Some(txn.clone());
                        for account in accessed_accounts {
                            account_last_position.insert(account.clone(), final_position);
                        }
                        while next_available_position < TXNS_PER_BATCH {
                            if scheduled[next_available_position].is_none() {
                                // println!("Next available position is now {}", next_available_position);
                                break;
                            }
                            next_available_position += 1;
                        }
                    }
                }
                None => {
                    println!("Failed to decode transaction: {:?}", txn);
                }
            }

            if available_slots == 0 {
                break;
            }
        }

        // println!("Scheduled {} transactions in this batch", scheduled_txn_indices.len());
        println!("Scheduled transaction indices: {:?}", scheduled_txn_indices);

        for &idx in scheduled_txn_indices.iter().rev() {
            if idx < txns.len() {
                txns.remove(idx);
            }
        }

        scheduled.retain(|x| x.is_some());
        scheduled
    }

    pub async fn fetch_batch(&self, n: usize) -> Vec<TransactionWithHint> {
        let mut lock = self.scheduled_txns.lock().await;
        let len = lock.len();
        let batch: Vec<TransactionWithHint> = lock.drain(..n.min(len)).collect();
        batch
    }

// Fair round-robin scheduler with per-caller sequential ordering
pub fn schedule_a_parallelizable_batch_fair_sequential(txns: &mut Vec<TransactionWithHint>) -> Vec<Option<TransactionWithHint>> {
    let concurrency_level: usize = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN).get();
    let TXNS_PER_BATCH: usize = concurrency_level * 4;
    let num_threads = 8;

    // Create per-caller queues for maintaining order
    let caller_queues = Arc::new(DashMap::<Address, Arc<StdMutex<VecDeque<(usize, TransactionWithHint)>>>>::new());
    let caller_processing = Arc::new(DashMap::<Address, bool>::new()); // Track which callers are being processed
    

    // Populate per-caller queues
    for (index, txn) in txns.iter().enumerate() {
        let queue = caller_queues
            .entry(txn.caller)
            .or_insert_with(|| Arc::new(StdMutex::new(VecDeque::new())))
            .clone();
        queue.lock().unwrap().push_back((index, txn.clone()));
    }


    // Round-robin work distribution: each "work item" is processing one transaction from one caller
    let injector = Arc::new(Injector::new());
    let workers: Vec<Worker<Address>> = (0..num_threads).map(|_| Worker::new_fifo()).collect();
    let stealers: Vec<Stealer<Address>> = workers.iter().map(|w| w.stealer()).collect();

    // Initially populate with all callers that have transactions
    for caller_entry in caller_queues.iter() {
        injector.push(*caller_entry.key());
    }

    // Shared scheduling state
    let account_being_accessed = Arc::new(StdMutex::new(HashSet::new()));
    let account_lock_mutex = Arc::new(StdMutex::new(()));
    let account_last_position = Arc::new(DashMap::new());
    let scheduled = Arc::new(StdMutex::new(vec![None; TXNS_PER_BATCH]));
    let scheduled_indices = Arc::new(StdMutex::new(Vec::<usize>::new()));
    let available_slots = Arc::new(AtomicUsize::new(TXNS_PER_BATCH));

    let handles: Vec<_> = workers.into_iter().enumerate()
        .map(|(thread_id, worker)| {
            let stealers = stealers.clone();
            let injector = injector.clone();
            let caller_queues = caller_queues.clone();
            let caller_processing = caller_processing.clone();
            let account_last_position = account_last_position.clone();
            let scheduled = scheduled.clone();
            let scheduled_indices = scheduled_indices.clone();
            let available_slots = available_slots.clone();
            let account_being_accessed = account_being_accessed.clone();
            let account_lock_mutex = account_lock_mutex.clone();

            thread::spawn(move || {
                let backoff = Backoff::new();
                
                loop {
                    // Try to get a caller to process ONE transaction from
                    let caller = worker.pop()
                        .or_else(|| injector.steal().success())
                        .or_else(|| {
                            stealers.iter()
                                .map(|s| s.steal())
                                .find(|s| s.is_success())
                                .and_then(|s| s.success())
                        });

                    match caller {
                        Some(caller) => {
                            // println!("📋 Thread {} processing caller {:?}", thread_id, caller);
                            backoff.reset();
                            
                            // Try to mark this caller as being processed (prevents double-processing)
                            if caller_processing.insert(caller, true).is_none() {
                                // We got the lock on this caller
                                let mut processed_transaction = false;
                                
                                if let Some(queue_ref) = caller_queues.get(&caller) {
                                    let mut queue = queue_ref.lock().unwrap();
                                    
                                    // Process exactly ONE transaction from this caller
                                    if let Some((index, txn)) = queue.pop_front() {
                                        processed_transaction = true;
                                        let mut position_found = false;

                                        if available_slots.load(Ordering::Acquire) > 0 {
                                            position_found = Self::process_transaction_sequential(
                                                thread_id,
                                                index,
                                                &txn,
                                                concurrency_level,
                                                TXNS_PER_BATCH,
                                                &account_last_position,
                                                &scheduled,
                                                &scheduled_indices,
                                                &available_slots,
                                                &account_being_accessed,
                                                &account_lock_mutex,
                                            );

                                            if !position_found {
                                                // println!("❌ [2] Thread {} could not schedule transaction {}", thread_id, index);
                                                queue.push_front((index, txn)); // Re-queue if not scheduled
                                            }
                                        }
                                        
                                        // Check if this caller has more work - if so, re-queue it
                                        if position_found && !queue.is_empty() && available_slots.load(Ordering::Acquire) > 0 {
                                            injector.push(caller); // Put caller back for next round
                                        }
                                    }
                                }
                                
                                // Release the caller processing lock
                                caller_processing.remove(&caller);
                                
                                if !processed_transaction {
                                    // Caller had no work, don't re-queue
                                    continue;
                                }
                            } else {
                                // Another thread is processing this caller, put it back
                                injector.push(caller);
                            }
                        }
                        None => {
                            break;
                            // No work available
                            if available_slots.load(Ordering::Acquire) == 0 {
                                break; // Batch is full
                            }
                            
                            // Check if any caller has remaining work
                            let has_remaining_work = caller_queues.iter()
                                .any(|entry| !entry.lock().unwrap().is_empty());
                            
                            if !has_remaining_work {
                                break; // No more work
                            }
                            
                            backoff.snooze();
                            if backoff.is_completed() {
                                // Re-populate injector in case we missed something
                                for caller_entry in caller_queues.iter() {
                                    let queue = caller_entry.lock().unwrap();
                                    if !queue.is_empty() && !caller_processing.contains_key(caller_entry.key()) {
                                        injector.push(*caller_entry.key());
                                    }
                                }
                                backoff.reset();
                            }
                        }

                    }
                }
            })
        })
        .collect();

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Extract and return results
    let scheduled_indices_result = scheduled_indices.lock().unwrap().clone();
    // println!("Scheduled transaction indices: {:?}", scheduled_indices_result);

    let mut sorted_indices = scheduled_indices_result.clone();
    sorted_indices.sort_by(|a, b| b.cmp(a));
    for idx in sorted_indices {
        if idx < txns.len() {
            txns.remove(idx);
        }
    }

    let mut scheduled_result = scheduled.lock().unwrap().clone();
    scheduled_result.retain(|x| x.is_some());
    scheduled_result
}


fn process_transaction_sequential(
    thread_id: usize,
    index: usize,
    txn: &TransactionWithHint,
    concurrency_level: usize,
    txns_per_batch: usize,
    account_last_position: &Arc<DashMap<AlloyAddress, usize>>,
    scheduled: &Arc<StdMutex<Vec<Option<TransactionWithHint>>>>,
    scheduled_indices: &Arc<StdMutex<Vec<usize>>>,
    available_slots: &AtomicUsize,
    account_being_accessed: &Arc<StdMutex<HashSet<AlloyAddress>>>,
    account_lock_mutex: &Arc<StdMutex<()>>,
) -> bool{
    let tx_env = deserializer::decode_one_hex(txn.raw_hex.clone(), txn.caller);
    let mut position_found = false;

    if let Some(tx_env) = tx_env {
        let mut accessed_accounts = Vec::new();
        
        match tx_env.transact_to {
            TransactTo::Call(to_addr) => {
                accessed_accounts.push(tx_env.caller);
                accessed_accounts.push(to_addr);
            }
            TransactTo::Create => {
                accessed_accounts.push(tx_env.caller);
            }
        }

        // println!("🔒 Thread {} processing transaction {} accessing accounts: {:?}", thread_id, index, accessed_accounts);

        let mut never_accessed = true;
        let mut out_of_bound = false;
        let mut final_position = 0;

        loop {
            let mut guard = account_being_accessed.lock().unwrap();
            if accessed_accounts.iter().all(|acc| !guard.contains(acc)) {
                for acc in &accessed_accounts {
                    guard.insert(*acc);
                }
                break; // No conflicts, proceed
            }
            drop(guard);
            thread::sleep(Duration::from_micros(100));  // Backoff before retrying
        }

        // println!("🔒 Thread {} Got All Account Locks {:?}", thread_id, accessed_accounts);

        // Find suitable position (same logic as original)
        for account in &accessed_accounts {
            if out_of_bound {
                break;
            }
            if let Some(last_pos_entry) = account_last_position.get(account) {
                never_accessed = false;
                let last_pos = *last_pos_entry.value();
                let mut new_position = last_pos + concurrency_level;

                if new_position <= final_position {
                    continue;
                } else {
                    position_found = false;
                    let scheduled_guard = scheduled.lock().unwrap();
                    while new_position < txns_per_batch {
                        if scheduled_guard[new_position].is_none() {
                            position_found = true;
                            final_position = new_position;
                            break;
                        }
                        new_position += 1;
                    }
                    if !position_found {
                        out_of_bound = true;
                    }
                    drop(scheduled_guard);
                }
            }
        }

        if never_accessed || position_found {
            let mut scheduled_guard = scheduled.lock().unwrap();
            if never_accessed {
                for i in 0..txns_per_batch {
                if scheduled_guard[i].is_none() {
                        position_found = true;
                        final_position = i;
                        break;
                    }
                }
            }

            if position_found {
                let mut indices_guard = scheduled_indices.lock().unwrap();

                if scheduled_guard[final_position].is_none() && 
                   available_slots.load(Ordering::Acquire) > 0 {

                    available_slots.fetch_sub(1, Ordering::AcqRel);
                    indices_guard.push(index);
                    scheduled_guard[final_position] = Some(txn.clone());
                    drop(scheduled_guard);
                    drop(indices_guard);
                    // Update account positions
                    for account in accessed_accounts.clone() {
                        account_last_position.insert(account, final_position);
                    }
                // println!("✅ [2] Thread {} scheduled transaction {} at position {}", thread_id, index, final_position);
                } else {
                    position_found = false;
                }
            }
        }
        let mut guard = account_being_accessed.lock().unwrap();
        for account in accessed_accounts.clone() {
            guard.remove(&account);
        }
        // println!("🔓 Thread {} Released All Account Locks with accounts {:?}", thread_id, accessed_accounts);
    } else {
        println!("Failed to decode transaction: {:?}", txn);
    }
    position_found
}

}

#[test]
pub fn store_in_memory_storage() {
    let (in_memory_storage, _account_addresses) = PevmAPI::get_erc20_state_and_bytecode(1, 2, 3);
    // Save
    save(&in_memory_storage, "storage.json");

    // Load
    let restored = load("storage.json");
    println!("Restored: {:?}", restored);
}

#[test]

pub fn store_account_address() {
    let (in_memory_storage, account_addresses) = PevmAPI::get_erc20_state_and_bytecode(1, 2, 3);
    // Save
    save_addresses("account_addresses.bin", &account_addresses);
    println!("Saved account addresses: {:?}", account_addresses);

    // Load
    let restored = load_addresses("account_addresses.bin");
    println!("Restored: {:?}", restored);
    
}

#[test]
pub fn store_and_load_both() {
    let (in_memory_storage, account_addresses) = PevmAPI::get_erc20_state_and_bytecode(8, 1, 8);

    let workload_type = WorkloadType::ERC20(8, 1, 8);

    let a1 = 8;
    let a2 = 1;
    let a3 = 8;

    let address_bin = format!("account_addresses_{}_{}_{}.bin", a1, a2, a3);
    let storage_json = format!("storage_{}_{}_{}.json", a1, a2, a3);

    // Save
    save_addresses(&address_bin, &account_addresses);

    save(&in_memory_storage, &storage_json);
    // println!("Saved in-memory storage: {:?}", in_memory_storage);

     // Load
    let restored_addresses = load_addresses(&address_bin);
    let restored = load(&storage_json);
    // println!("Restored: {:?}", restored);

    assert_eq!(restored.unwrap(), in_memory_storage);
    assert_eq!(restored_addresses.unwrap(), account_addresses);
}


#[test]

pub fn test_load_both() {
    let workload_type = WorkloadType::ERC20(1, 1, 4);

    let a1 = 1;
    let a2 = 1;
    let a3 = 4;

    let address_bin = format!("account_addresses_{}_{}_{}.bin", a1, a2, a3);
    let storage_json = format!("storage_{}_{}_{}.json", a1, a2, a3);

    let restored_addresses = load_addresses("account_addresses.bin");
    println!("Restored: {:?}", restored_addresses);
    let restored_storage = load("storage.json").unwrap();
    // println!("Restored: {:?}", restored);

    let (tx1, rx1) = mpsc::channel(100);
    let (tx2, rx2) = mpsc::channel(100);

    let mut generator = PevmTransactionGenerator::new(workload_type, 1u64, 4u64, tx1, rx2);

    let transactions = generator.generate_transactions();

    let tx_envs = deserializer::decode_batch_hex(transactions);

    let chain = PevmEthereum::mainnet();

    println!("tx_envs: {:?}", tx_envs);

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    
    tracing::info!("Executed transactions in parallel with {} threads", concurrency_level);
    let results = Pevm::default().execute_revm_parallel(
        &chain,
        &restored_storage,
        SpecId::LATEST,
        BlockEnv::default(),
        tx_envs,
        concurrency_level,
    );

    // println!("Results: {:?}", results);

}

#[test]
pub fn test_load_in_memory_storage(){
    let workload_type = WorkloadType::ERC20(1, 2, 3);
    let storage = load_in_memory_storage(&workload_type);
    let addresses = load_account_addresses(&workload_type);
    println!("{:?}", addresses);
}


#[test]
pub fn test_max_throughput_parallel() {
    let workload_type = WorkloadType::ERC20(1, 1, 4);

    let a1 = 1;
    let a2 = 1;
    let a3 = 4;

    let address_bin = format!("account_addresses_{}_{}_{}.bin", a1, a2, a3);
    let storage_json = format!("storage_{}_{}_{}.json", a1, a2, a3);

    let restored_addresses = load_addresses(&address_bin);
    // println!("Restored: {:?}", restored_addresses);
    let restored_storage = load(&storage_json).unwrap();

    let (tx1, rx1) = mpsc::channel(100);
    let (tx2, rx2) = mpsc::channel(100);

    let mut generator = PevmTransactionGenerator::new(workload_type, 1u64, 4u64, tx1, rx2);

    let mut all_tx_env = Vec::new();

    loop {
        let transactions = generator.generate_transactions();
        let tx_envs = deserializer::decode_batch_hex(transactions);
        all_tx_env.extend(tx_envs);
        if all_tx_env.len() >= 10000 {
            break;
        }
    }

    let num_tx_env = all_tx_env.len();

    let chain = PevmEthereum::mainnet();

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);

    let concurrency_level = NonZeroUsize::new(8).unwrap();

    println!("Executed transactions in parallel with {} threads", concurrency_level);

    let start = Instant::now();
    let results = Pevm::default().execute_revm_parallel(
            &chain,
            &restored_storage,
            SpecId::LATEST,
            BlockEnv::default(),
            all_tx_env,
            concurrency_level,
        );

    

    let elapsed: Duration = start.elapsed();

    let secs_f64: f64 = elapsed.as_secs_f64();

    println!("Elapsed = {} seconds (f64)", secs_f64);

    println!("Throughput = {}", num_tx_env as f64 / secs_f64);

}

#[test]
pub fn test_max_throughput_sequential() {
    let workload_type = WorkloadType::ERC20(1, 1, 4);

    let a1 = 1;
    let a2 = 1;
    let a3 = 4;

    let address_bin = format!("account_addresses_{}_{}_{}.bin", a1, a2, a3);
    let storage_json = format!("storage_{}_{}_{}.json", a1, a2, a3);

    let restored_addresses = load_addresses(&address_bin);
    // println!("Restored: {:?}", restored_addresses);
    let restored_storage = load(&storage_json).unwrap();

    let (tx1, rx1) = mpsc::channel(100);
    let (tx2, rx2) = mpsc::channel(100);

    let mut generator = PevmTransactionGenerator::new(workload_type, 1u64, 4u64, tx1, rx2);

    let mut all_tx_env = Vec::new();

    loop {
        let transactions = generator.generate_transactions();
        let tx_envs = deserializer::decode_batch_hex(transactions);
        all_tx_env.extend(tx_envs);
        if all_tx_env.len() >= 10000 {
            break;
        }
    }

    let num_tx_env = all_tx_env.len();

    let chain = PevmEthereum::mainnet();

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);

    let start = Instant::now();
    let results = super::pevm::execute_revm_sequential(
            &chain,
            &restored_storage,
            SpecId::LATEST,
            BlockEnv::default(),
            all_tx_env,
        );

    let elapsed: Duration = start.elapsed();

    let secs_f64: f64 = elapsed.as_secs_f64();

    println!("Elapsed = {} seconds (f64)", secs_f64);

    println!("Throughput = {}", num_tx_env as f64 / secs_f64);

    let (tx1, rx1) = mpsc::channel(100);
    let (tx2, rx2) = mpsc::channel(100);

    let workload_type = WorkloadType::ERC20(1, 1, 4);

    let mut generator = PevmTransactionGenerator::new(workload_type, 1u64, 4u64, tx1, rx2);

    let mut all_tx_env = Vec::new();

    loop {
        let transactions = generator.generate_transactions();
        let tx_envs = deserializer::decode_batch_hex(transactions);
        all_tx_env.extend(tx_envs);
        if all_tx_env.len() >= 10000 {
            break;
        }
    }

    let num_tx_env = all_tx_env.len();

    let chain = PevmEthereum::mainnet();

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);

    let start = Instant::now();
    let results = super::pevm::execute_revm_sequential(
            &chain,
            &restored_storage,
            SpecId::LATEST,
            BlockEnv::default(),
            all_tx_env,
        );

    let elapsed: Duration = start.elapsed();

    let secs_f64: f64 = elapsed.as_secs_f64();

    println!("Elapsed = {} seconds (f64)", secs_f64);

    println!("Throughput = {}", num_tx_env as f64 / secs_f64);

}

#[test]
pub fn test_scheduling() {
    let workload_type = WorkloadType::ERC20(8, 1, 8);
    let mut restored_storage = load_in_memory_storage(&workload_type);
    let chain = PevmEthereum::mainnet();
    let mut generator = PevmTransactionGenerator::new(workload_type, 0u64, 4u64, mpsc::channel(100).0, mpsc::channel(100).1);
    let mut transactions = Vec::new();
    for i in 0..10 {
        let mut batch = generator.generate_contended_erc20_transactions();
        transactions.append(&mut batch);
        for j in 0..1 {
            let mut batch = generator.generate_parallelizable_erc20_transactions();
            transactions.append(&mut batch);
        }
    }

    let mut transactions_with_hint: Vec<TransactionWithHint> = transactions.iter().map(|(raw_hex, caller)| TransactionWithHint {
        raw_hex: raw_hex.clone(),
        caller: *caller,
        hint: String::new(),
    }).collect();

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
     println!("Total number of transactions: {}", transactions_with_hint.len());
    let start = Instant::now();
    let mut total_execution_time = 0;
    while !transactions_with_hint.is_empty() {
        
        //  println!("Remaining number of transactions: {}", transactions_with_hint.len());
        let batch = PevmScheduler::schedule_a_parallelizable_batch(&mut transactions_with_hint);

        let time1 = Instant::now();
        // println!("Batch size: {}", batch.len());
        let tx_envs: Vec<TxEnv> = batch.iter().filter_map(|opt_txn| {
            if let Some(txn) = opt_txn {
                deserializer::decode_one_hex(txn.raw_hex.clone(), txn.caller)
            } else {
                None
            }
        }).collect();

        let results = Pevm::default().execute_revm_parallel(
            &chain,
            &restored_storage,
            SpecId::LATEST,
            BlockEnv::default(),
            tx_envs,
            concurrency_level,
        );
        let elapsed: Duration = time1.elapsed();

        update_storage_with_results(&mut restored_storage, results.unwrap());
        
        total_execution_time += elapsed.as_millis();
    }

    let elapsed: Duration = start.elapsed();
    println!("Elapsed = {} seconds (f64)", elapsed.as_secs_f64());
    println!("Total execution time in ms = {}", total_execution_time);
}


#[test]
pub fn test_parallel_scheduling() {
    let workload_type = WorkloadType::ERC20(8, 1, 8);
    let mut restored_storage = load_in_memory_storage(&workload_type);
    let chain = PevmEthereum::mainnet();
    let mut generator = PevmTransactionGenerator::new(workload_type, 0u64, 4u64, mpsc::channel(100).0, mpsc::channel(100).1);
    let mut transactions = Vec::new();
    for i in 0..10 {
        let mut batch = generator.generate_contended_erc20_transactions();
        transactions.append(&mut batch);
        for j in 0..1 {
            let mut batch = generator.generate_parallelizable_erc20_transactions();
            transactions.append(&mut batch);
        }
    }

    let mut transactions_with_hint: Vec<TransactionWithHint> = transactions.iter().map(|(raw_hex, caller)| TransactionWithHint {
        raw_hex: raw_hex.clone(),
        caller: *caller,
        hint: String::new(),
    }).collect();

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
     println!("Total number of transactions: {}", transactions_with_hint.len());
    let start = Instant::now();
    let mut total_execution_time = 0;
    while !transactions_with_hint.is_empty() {
         println!("Remaining number of transactions: {}", transactions_with_hint.len());
        let batch = PevmScheduler::schedule_a_parallelizable_batch_fair_sequential(&mut transactions_with_hint);

        println!("Batch size after fair scheduling: {}", batch.len());

        let time1 = Instant::now();
        let tx_envs: Vec<TxEnv> = batch.iter().filter_map(|opt_txn| {
            if let Some(txn) = opt_txn {
                deserializer::decode_one_hex(txn.raw_hex.clone(), txn.caller)
            } else {
                None
            }
        }).collect();


        // for txn in tx_envs.iter() {
            // println!("Decoded TxEnv: caller={:?}, nonce={}", txn.caller, txn.nonce.unwrap_or(0));
        // }

        let results = Pevm::default().execute_revm_parallel(
            &chain,
            &restored_storage,
            SpecId::LATEST,
            BlockEnv::default(),
            tx_envs,
            concurrency_level,
        );

        println!("Executed transactions in this batch");

        let elapsed: Duration = time1.elapsed();

        update_storage_with_results(&mut restored_storage, results.unwrap());
        
        total_execution_time += elapsed.as_millis();
    }

    let elapsed: Duration = start.elapsed();
    println!("Elapsed = {} seconds (f64)", elapsed.as_secs_f64());
    println!("Total execution time in ms = {}", total_execution_time);
}

#[test]

pub fn test_no_scheduling() {
    let workload_type = WorkloadType::ERC20(8, 1, 8);
    let mut restored_storage = load_in_memory_storage(&workload_type);
    let chain = PevmEthereum::mainnet();
    let mut generator = PevmTransactionGenerator::new(workload_type, 0u64, 4u64, mpsc::channel(100).0, mpsc::channel(100).1);
    let mut transactions = Vec::new();

    for i in 0..10 {
        let mut batch = generator.generate_contended_erc20_transactions();
        transactions.append(&mut batch);
        for j in 0..1 {
            let mut batch = generator.generate_parallelizable_erc20_transactions();
            transactions.append(&mut batch);
        }
    }

    let mut transactions_with_hint: Vec<TransactionWithHint> = transactions.iter().map(|(raw_hex, caller)| TransactionWithHint {
        raw_hex: raw_hex.clone(),
        caller: *caller,
        hint: String::new(),
    }).collect();

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
     println!("Total number of transactions: {}", transactions_with_hint.len());
    let start = Instant::now();
    let mut total_execution_time = 0;
    while !transactions_with_hint.is_empty() {
        
        //  println!("Remaining number of transactions: {}", transactions_with_hint.len());
        let batch: Vec<_> = transactions_with_hint
            .drain(..32.min(transactions_with_hint.len()))
            .collect();


        let time1 = Instant::now();
        // println!("Batch size: {}", batch.len());
        let tx_envs: Vec<TxEnv> = batch.iter().map(|txn| {
            deserializer::decode_one_hex(txn.raw_hex.clone(), txn.caller).unwrap()
        }).collect();

        let results = Pevm::default().execute_revm_parallel(
            &chain,
            &restored_storage,
            SpecId::LATEST,
            BlockEnv::default(),
            tx_envs,
            concurrency_level,
        );
        let elapsed: Duration = time1.elapsed();

        update_storage_with_results(& mut restored_storage, results.unwrap());
        
        total_execution_time += elapsed.as_millis();
    }

    let elapsed: Duration = start.elapsed();
    println!("Elapsed = {} seconds (f64)", elapsed.as_secs_f64());
    println!("Total execution time in ms = {}", total_execution_time);
}


pub fn update_storage_with_results(storage: &mut InMemoryStorage, results: Vec<PevmTxExecutionResult>) {
        let mut state = storage.accounts_clone();
        for changed_state in results.iter() {
            for (addr, acc) in changed_state.state.iter() {
                match acc {
                    Some(account) => {
                        state.insert(*addr, account.clone());
                    }
                    None => {
                        state.remove(addr);
                    }
                }
            }
        }
        storage.update_accounts(state);
}

#[test]

pub fn test_contended_workload() {
    let workload_type = WorkloadType::ERC20(5, 5, 8);
    let restored_storage = load_in_memory_storage(&workload_type);
    let chain = PevmEthereum::mainnet();
    let mut generator = PevmTransactionGenerator::new(workload_type, 0u64, 4u64, mpsc::channel(100).0, mpsc::channel(100).1);
    let mut transactions = generator.generate_contended_erc20_transactions();
    let mut parallelizable_transactions = generator.generate_parallelizable_erc20_transactions();

    let tx_envs = deserializer::decode_batch_hex(transactions);
    let parallelizable_tx_envs = deserializer::decode_batch_hex(parallelizable_transactions);

    let concurrency_level = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    
    println!("Executed trasactions in parallel with {} threads", concurrency_level);

    println!("Executing {} transactions", tx_envs.len());

    // println!("{:#?}", tx_envs);

    let start = Instant::now();

    let results = Pevm::default().execute_revm_parallel(
        &chain,
        &restored_storage,
        SpecId::LATEST,
        BlockEnv::default(),
        tx_envs,
        concurrency_level,
    );

    let elapsed: Duration = start.elapsed();
    println!("Elapsed = {} seconds (f64)", elapsed.as_secs_f64());

}
