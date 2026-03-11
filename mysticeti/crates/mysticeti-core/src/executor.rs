use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

pub use ethers::types::Address;
use pevm::api::{PevmExecutor, TransactionWithHint};

use crate::{
    consensus::linearizer::CommittedSubDag,
    metrics::Metrics,
    runtime::{self},
    types::BaseStatement,
};

pub struct Executor {
    /// The execution engine, states are in memory
    pub pevm_executor: PevmExecutor,
    /// The receiver of committed sub-dags from core
    pub committed_message_receiver: mpsc::Receiver<Vec<CommittedSubDag>>,
    metrics: Arc<Metrics>,
}

impl Executor {
    pub fn start(
        pevm_executor: PevmExecutor,
        committed_message_receiver: mpsc::Receiver<Vec<CommittedSubDag>>,
        metrics: Arc<Metrics>,
    ) {
        runtime::Handle::current().spawn(async move {
            Self {
                pevm_executor,
                committed_message_receiver,
                metrics,
            }
            .run()
            .await;
        });
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(committed_blocks) = self.committed_message_receiver.recv() => {
                    tracing::debug!("Received {} ordered blocks to execute", committed_blocks.len());
                    let mut block_num = 0;
                    let mut txs = Vec::<(String, Address)>::new();
                    let mut txs_creation_timestamp = Vec::<Duration>::new();
                    for sub_dag in committed_blocks.iter() {
                        for block in sub_dag.blocks.iter() {
                            block_num += 1;
                            for statement in block.statements() {
                                if let BaseStatement::Share(share) = statement {
                                    let (creation_time, raw_hex, caller) = decode_share_base_statement(share.data());
                                    txs.push((raw_hex, caller));
                                    txs_creation_timestamp.push(creation_time);
                                }
                            }
                        }
                    }

                    let start_execution_time = std::time::Instant::now();
                    if txs.len() > 0 {
                        self.pevm_executor.execute(txs);
                    }
                    let end_execution_time = start_execution_time.elapsed();
                    // Record block execution latency
                    self.metrics.block_execution_latency.observe(end_execution_time / block_num);

                    tracing::debug!("Block execution time: {:?}", end_execution_time);

                    // Record end-to-end transaction latency.
                    let current_timestamp = runtime::timestamp_utc();
                    for creation_time in txs_creation_timestamp.iter() {
                        self.metrics.transaction_committed_latency.observe(current_timestamp.saturating_sub(*creation_time));
                    }

                }
                else => {
                    tracing::debug!("Channel closed");
                    break;
                }
            }
        }
    }
}
fn decode_share_base_statement(data: &[u8]) -> (Duration, String, Address) {
    let decoded: TransactionWithHint = bincode::deserialize(&data).unwrap();
    let time = decoded.timestamp;
    let creation_timestamp = Duration::from_millis(u64::from_le_bytes(time));
    let raw_hex = decoded.raw_hex;
    let caller = decoded.caller;
    (creation_timestamp, raw_hex, caller)
}
