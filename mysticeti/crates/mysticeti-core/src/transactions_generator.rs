// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{cmp::min, sync::Arc, time::Duration};

pub use ethers::types::Address;
use pevm::api::{APIError, PevmAPI, PevmScheduler, PevmTransactionGenerator, TransactionWithHint};
use pevm::serialization::deserializer;
use rand::{rngs::StdRng, Rng, SeedableRng};
use tokio::sync::{mpsc, Mutex};
use tokio::task;
use tokio::time::sleep;

use crate::{
    config::{ClientParameters, NodePublicConfig},
    crypto::AsBytes,
    metrics::Metrics,
    runtime::{self, timestamp_utc},
    types::{AuthorityIndex, BaseStatement, Transaction},
};

pub struct TransactionGenerator {
    sender: mpsc::Sender<Vec<Transaction>>,
    rng: StdRng,
    client_parameters: ClientParameters,
    node_public_config: NodePublicConfig,
    metrics: Arc<Metrics>,
}

impl TransactionGenerator {
    const TARGET_BLOCK_INTERVAL: Duration = Duration::from_millis(100);

    pub fn start(
        sender: mpsc::Sender<Vec<Transaction>>,
        seed: AuthorityIndex,
        client_parameters: ClientParameters,
        node_public_config: NodePublicConfig,
        metrics: Arc<Metrics>,
        pevm_scheduler: Arc<PevmScheduler>,
        insufficient_txn_signal_sender: mpsc::Sender<usize>,
    ) {
        assert!(client_parameters.transaction_size > 8 + 8); // 8 bytes timestamp + 8 bytes random
        tracing::info!(
            "Starting generator with {} transactions per second, initial delay {:?}",
            client_parameters.load,
            client_parameters.initial_delay
        );

        let committee_size = *(&node_public_config.identifiers.len()) as u64;
        let workload_type = node_public_config.parameters.pevm_workload_type.clone();

        runtime::Handle::current().spawn(
            Self {
                sender,
                rng: StdRng::seed_from_u64(seed),
                client_parameters,
                node_public_config,
                metrics,
            }
            .run(pevm_scheduler, seed, insufficient_txn_signal_sender),
        );
    }

    pub async fn run(
        mut self,
        pevm_scheduler: Arc<PevmScheduler>,
        id: AuthorityIndex,
        insufficient_txn_signal_sender: mpsc::Sender<usize>,
    ) {
        let load = self.client_parameters.load;
        let transactions_per_block_interval = (load + 9) / 10;
        tracing::info!(
            "Generating {transactions_per_block_interval} transactions per {} ms",
            Self::TARGET_BLOCK_INTERVAL.as_millis()
        );
        let max_block_size = self.node_public_config.parameters.max_block_size;
        let target_block_size = min(max_block_size, transactions_per_block_interval);

        let mut counter = 0;
        let mut tx_to_report = 0;
        // let mut random: u64 = self.rng.gen(); // 8 bytes
        // let zeros = vec![0u8; self.client_parameters.transaction_size - 8 - 8]; // 8 bytes timestamp + 8 bytes random

        let mut interval = runtime::TimeInterval::new(Self::TARGET_BLOCK_INTERVAL);
        runtime::sleep(self.client_parameters.initial_delay).await;
        loop {
            interval.tick().await;
            let timestamp = (timestamp_utc().as_millis() as u64).to_le_bytes();
            let mut block = Vec::with_capacity(target_block_size);
            let mut block_size = 0;
            let mut x = 0;
            for _ in 0..transactions_per_block_interval {
                let batch = pevm_scheduler.fetch_batch(1).await;
                let mut fetched_txn = if let Some(txn) = batch.into_iter().next() {
                    // tracing::debug!("fetched {}-th txn: {:?}", &x, &txn);
                    x += 1;
                    txn
                } else {
                    continue;
                };

                fetched_txn.timestamp = timestamp;
                let transaction: Vec<u8> = bincode::serialize(&fetched_txn).unwrap();

                block.push(Transaction::new(transaction));
                block_size += self.client_parameters.transaction_size;

                // tracing::debug!(
                //     "Block_size = {}, max_block_size = {}",
                //     block_size,
                //     max_block_size
                // );

                counter += 1;
                tx_to_report += 1;

                if block_size >= max_block_size {
                    // tracing::debug!("block size is full: {}", block_size);
                    break;
                }
            }

            insufficient_txn_signal_sender
                .send(transactions_per_block_interval)
                .await;
            if !block.is_empty() && self.sender.send(block).await.is_err() {
                return;
            }

            if counter % 10_000 == 0 {
                self.metrics.submitted_transactions.inc_by(tx_to_report);
                tx_to_report = 0
            }
        }
    }

    pub fn extract_timestamp(transaction: &Transaction) -> Duration {
        let bytes = transaction.as_bytes()[0..8]
            .try_into()
            .expect("Transactions should be at least 8 bytes");
        Duration::from_millis(u64::from_le_bytes(bytes))
    }

    pub async fn read_workload_from_file(
        pevm_api: Arc<Mutex<PevmAPI>>,
        file_path: &str,
    ) -> Result<(), APIError> {
        let mut reader = match deserializer::ChunkFileReader::open(file_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Error opening workload file {}: {}", file_path, e);
                return Err(APIError::NoWorkloadFile);
            }
        };
        // Read two chunks of 10 lines each
        loop {
            let num_scheduled_txn = {
                let api_guard = pevm_api.lock().await; // keep the outer guard alive
                let mut txns_queue = api_guard.txns_queue.lock().await;
                let mut scheduled_queue = api_guard.scheduled_txns.lock().await;
                txns_queue.len() + scheduled_queue.len()
            };
            if num_scheduled_txn < 100 {
                let batch = reader.read_next(100);
                match &batch {
                    Ok(b) => {
                        if b.is_empty() {
                            return Ok(());
                        } else {
                            println!("Read {} transactions from workload file", b.len());
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading workload file {}: {}", file_path, e);
                        return Err(APIError::NoWorkloadFile);
                    }
                };
                let txs = batch
                    .unwrap()
                    .into_iter()
                    .map(|(raw_hex, caller)| {
                        TransactionWithHint {
                            raw_hex,
                            caller,
                            hint: String::new(), // [TODO] Placeholder
                            timestamp: [0u8; 8],
                        }
                    })
                    .collect();
                pevm_api.lock().await.add_transactions(txs).await;
            } else {
                sleep(Duration::from_millis(10)).await;
            }
        }
    }

    pub async fn schedule(pevm_api: Arc<Mutex<PevmAPI>>) {
        let mut empty = false;
        loop {
            if empty {
                sleep(Duration::from_millis(10)).await;
            }

            let front_item = {
                let guard = pevm_api.lock().await;
                let mut queue = guard.txns_queue.lock().await;
                queue.pop_front()
            };

            if let Some(transaction) = front_item {
                empty = false;
                // tracing::info!("Scheduling transaction: {:?}", transaction);
                // do_some_scheduling_work(transaction).await;
                let guard = pevm_api.lock().await;
                // TODO: Wrap scheduled_txns in a Arc<Mutex<>> to release the guard earlier
                let mut scheduled_queue = guard.scheduled_txns.lock().await;
                scheduled_queue.push_back(transaction);
            } else {
                empty = true;
                continue;
            }
        }
    }
}

fn split_bytes_by_pipe(data: &[u8]) -> Vec<Vec<u8>> {
    data.split(|&b| b == b'|')
        .map(|chunk| chunk.to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[tokio::test]
    // async fn test_read_workload_file() {
    //     let id: AuthorityIndex = 0;
    //     let path = std::env::current_dir().unwrap();
    //     println!("Current path: {}", path.display());
    //     println!("Reading workload file for id {}", id);
    //     let file_path = "/home/ubuntu/congestion_control/pevm/crates/pevm/workload_{}.txt".replace("{}", &id.to_string());
    //     let pevm_api = Arc::new(Mutex::new(PevmAPI::new()));
    //     let handle = task::spawn( async move {
    //             TransactionGenerator::read_workload_from_file(pevm_api.clone(), &file_path).await;
    //         }
    //     );
    //     handle.await.unwrap();
    // }

    #[test]
    fn test_decode_transaction() {
        let mut transaction = Vec::with_capacity(512);
        transaction.extend_from_slice(&123u64.to_be_bytes()); // 8 bytes
        transaction.push(b'|');
        let raw_hex = String::from("0x1234567890abcdef");
        transaction.extend_from_slice(raw_hex.as_bytes());
        transaction.push(b'|');
        let caller: Address = "0xabcdef1234567890abcdef1234567890abcdef12"
            .parse()
            .expect("Invalid address");
        transaction.extend_from_slice(caller.as_bytes());
        transaction.push(b'|');

        let statement = BaseStatement::Share(Transaction::new(transaction));

        if let BaseStatement::Share(data) = statement {
            let parts = split_bytes_by_pipe(data.data());
            let raw_hex = String::from_utf8_lossy(&parts[1]);
            println!("Raw hex: {}", raw_hex);
            let caller_bytes: [u8; 20] = parts[2]
                .as_slice()
                .try_into()
                .expect("address must be 20 bytes");
            let caller = Address::from(caller_bytes);
            println!("Caller: {:?}", caller);
        }
    }

    #[test]
    fn test_decode_txn_with_hint() {
        let tx = TransactionWithHint {
            raw_hex: String::from("0x1234567890abcdef"),
            caller: "0xabcdef1234567890abcdef1234567890abcdef12"
                .parse()
                .expect("Invalid address"),
            hint: String::from(""),
            timestamp: [0u8; 8],
        };
        let encoded: Vec<u8> = bincode::serialize(&tx).unwrap();
        let decoded: TransactionWithHint = bincode::deserialize(&encoded).unwrap();
        println!("decoded: {:?}", decoded);
    }
}
