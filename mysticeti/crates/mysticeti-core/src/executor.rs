use tokio::sync::mpsc;

pub use ethers::types::Address;
use pevm::api::{PevmExecutor, TransactionWithHint};

use crate::{
    data::Data,
    runtime::{self},
    types::{BaseStatement, StatementBlock},
};

pub struct Executor {
    /// The execution engine, states are in memory
    pub pevm_executor: PevmExecutor,
    /// The ordered blocks waiting to be executed
    pub pending_blocks: Vec<Data<StatementBlock>>,
    /// The receiver of ordered blocks from core
    pub ordered_txns_receiver: mpsc::Receiver<Vec<Data<StatementBlock>>>,
}

impl Executor {
    pub fn start(
        pevm_executor: PevmExecutor,
        ordered_txns_receiver: mpsc::Receiver<Vec<Data<StatementBlock>>>,
    ) {
        runtime::Handle::current().spawn(async move {
            Self {
                pevm_executor,
                pending_blocks: Vec::new(),
                ordered_txns_receiver,
            }
            .run()
            .await;
        });
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(ordered_blocks) = self.ordered_txns_receiver.recv() => {
                    tracing::info!("Received {} ordered blocks to execute", ordered_blocks.len());
                    self.pending_blocks.extend(ordered_blocks);

                    // Process pending blocks immediately after receiving new ones
                    self.process_pending_blocks().await;
                }
                else => {
                    // Channel is closed, process any remaining blocks and exit
                    tracing::info!("Channel closed, processing remaining {} blocks", self.pending_blocks.len());
                    self.process_pending_blocks().await;
                    break;
                }
            }
        }
    }

    async fn process_pending_blocks(&mut self) {
        while let Some(block) = self.pending_blocks.first() {
            // Extract transactions from the block
            let mut txs = Vec::<(String, Address)>::new();
            for statement in block.statements() {
                if let BaseStatement::Share(share) = statement {
                    let (raw_hex, caller) = decode_share_base_statement(share.data());
                    txs.push((raw_hex, caller));
                }
            }
            if txs.len() > 0 {
                tracing::info!("Executing {} transactions in pevm", txs.len());
                self.pevm_executor.execute(txs);
            }

            self.pending_blocks.remove(0);
        }
    }
}
fn decode_share_base_statement(data: &[u8]) -> (String, Address) {
    let decoded: TransactionWithHint = bincode::deserialize(&data).unwrap();
    let raw_hex = decoded.raw_hex;
    let caller = decoded.caller;
    (raw_hex, caller)
}
