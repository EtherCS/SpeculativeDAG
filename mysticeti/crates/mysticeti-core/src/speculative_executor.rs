use tokio::sync::mpsc;

pub use ethers::types::Address;
use pevm::api::{PevmExecutor, TransactionWithHint};

use crate::{
    data::Data,
    runtime::{self},
    types::{
        APSTree, BaseStatement, EvmStateWriteSet, RoundNumber, SpeculativeExecutionSnapshot,
        StatementBlock,
    },
};

pub enum SpeculativeMessage {
    /// (speculative order, last round that is predicted to be committed, the new ordered blocks via speculation)
    /// The last_round is used to find the corresponding snapshot
    SpeculativeExecuteTxs(APSTree, RoundNumber, Vec<Data<StatementBlock>>),
    CommitLeaders(Vec<Data<StatementBlock>>), // the ordered leader blocks via consensus
    CommittedExecuteResults(EvmStateWriteSet), // the committed execution results from core
}

pub struct SpeculativeExecutor {
    /// The execution engine, states are in memory
    pub pevm_executor: PevmExecutor,
    /// The committed blocks waiting to be persist into storage
    pub pending_committed_leader_blocks: Vec<Data<StatementBlock>>,
    /// The receiver of speculatively ordered blocks from core
    pub speculative_message_receiver: mpsc::Receiver<SpeculativeMessage>,
    /// The maintained speculative execution snapshots
    pub snapshots: Vec<SpeculativeExecutionSnapshot>,
}

impl SpeculativeExecutor {
    pub fn start(
        pevm_executor: PevmExecutor,
        speculative_message_receiver: mpsc::Receiver<SpeculativeMessage>,
    ) {
        runtime::Handle::current().spawn(async move {
            // Create an initial empty snapshot as the base state
            let initial_snapshot = SpeculativeExecutionSnapshot::new(
                Vec::new(),                  // No leaders yet
                EvmStateWriteSet::default(), // Empty initial state
            );

            Self {
                pevm_executor,
                pending_committed_leader_blocks: Vec::new(),
                speculative_message_receiver,
                snapshots: vec![initial_snapshot], // Start with base snapshot
            }
            .run()
            .await;
        });
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(speculative_message) = self.speculative_message_receiver.recv() => {
                    match speculative_message{
                        SpeculativeMessage::SpeculativeExecuteTxs(aps_tree, last_predict_round, ordered_blocks) => {
                            tracing::debug!("Received {} speculatively ordered blocks to execute", ordered_blocks.len());
                            // extract the old committed leader blocks
                            let old_committed_leaders = aps_tree.get_predict_committed_leader_blocks_up_to_round(last_predict_round);
                            let new_committed_leaders = aps_tree.get_predict_committed_leader_blocks_since_round(last_predict_round+1);
                            self.speculative_execution_on_blocks(old_committed_leaders, new_committed_leaders, ordered_blocks).await;
                        },
                        SpeculativeMessage::CommitLeaders(ordered_blocks) => {
                            tracing::debug!("Received {} committed ordered blocks to execute", ordered_blocks.len());
                            // self.pending_blocks.extend(ordered_blocks);
                        },
                        SpeculativeMessage::CommittedExecuteResults(evm_state_write_set) => {
                            tracing::debug!("Received committed execution results from core");
                            // update the pevm_executor state with the committed execution results
                            // self.pevm_executor.apply_state_write_set(evm_state_write_set);
                        },
                    }
                }
                else => {
                    // Channel is closed, process any remaining blocks and exit
                    tracing::info!("Channel closed, processing remaining {} blocks", self.pending_committed_leader_blocks.len());
                    // self.process_pending_blocks().await;
                    break;
                }
            }
        }
    }

    // perform speculative execution on the ordered blocks, given the old committed leader blocks
    async fn speculative_execution_on_blocks(
        &mut self,
        old_committed_leaders: Vec<Data<StatementBlock>>,
        new_committed_leaders: Vec<Data<StatementBlock>>,
        ordered_blocks: Vec<Data<StatementBlock>>,
    ) {
        // Find the best matching snapshot (longest common prefix with old_committed_leaders)
        let best_snapshot = self.find_best_matching_snapshot(&old_committed_leaders);

        match best_snapshot {
            Some(snapshot) => {
                tracing::debug!(
                    "Found matching snapshot with {} leaders, executing {} new blocks on top",
                    snapshot.ordered_leaders.len(),
                    ordered_blocks.len()
                );

                // Extract transactions from the new ordered blocks
                let mut txs = Vec::<(String, Address)>::new();
                for block in &ordered_blocks {
                    for statement in block.statements() {
                        if let BaseStatement::Share(share) = statement {
                            let (raw_hex, caller) = decode_share_base_statement(share.data());
                            txs.push((raw_hex, caller));
                        }
                    }
                }
                // speculatively execute the new transactions on top of the snapshot state
                let snapshot_transition_states = snapshot.transition_states.clone();
                let new_state = self
                    .pevm_executor
                    .speculative_execute(txs, snapshot_transition_states);

                // todo: reputation mechanism
                // Create new snapshot with combined order
                let mut new_ordered_leaders = old_committed_leaders.clone();
                new_ordered_leaders.extend(new_committed_leaders.clone());

                let new_snapshot =
                    SpeculativeExecutionSnapshot::new(new_ordered_leaders, new_state);

                // Add or update the snapshot
                self.add_or_update_snapshot(new_snapshot);
            }
            None => {
                tracing::warn!(
                    "No matching snapshot found for {} old committed leaders, cannot execute speculatively",
                    old_committed_leaders.len()
                );
            }
        }
    }

    /// Find the snapshot with the longest common prefix matching the given leaders
    fn find_best_matching_snapshot(
        &self,
        target_leaders: &[Data<StatementBlock>],
    ) -> Option<&SpeculativeExecutionSnapshot> {
        let mut best_match: Option<&SpeculativeExecutionSnapshot> = None;
        let mut best_match_length = 0;

        for snapshot in self.snapshots.iter().rev() {
            let common_prefix_len =
                self.common_prefix_length(&snapshot.ordered_leaders, target_leaders);

            if common_prefix_len == target_leaders.len() {
                // Perfect match
                return Some(snapshot);
            }
            // The snapshot must be a complete prefix of target_leaders
            if common_prefix_len == snapshot.ordered_leaders.len()
                && common_prefix_len > best_match_length
            {
                best_match = Some(snapshot);
                best_match_length = common_prefix_len;
            }
        }

        best_match
    }

    /// Calculate the length of the common prefix between two leader sequences
    fn common_prefix_length(
        &self,
        leaders_a: &[Data<StatementBlock>],
        leaders_b: &[Data<StatementBlock>],
    ) -> usize {
        leaders_a
            .iter()
            .zip(leaders_b.iter())
            .take_while(|(a, b)| a.digest() == b.digest())
            .count()
    }

    /// Add a new snapshot or update an existing one with the same order
    fn add_or_update_snapshot(&mut self, new_snapshot: SpeculativeExecutionSnapshot) {
        // Check if a snapshot with the same ordered_leaders already exists
        if let Some(pos) = self.snapshots.iter().position(|s| {
            s.ordered_leaders.len() == new_snapshot.ordered_leaders.len()
                && self.common_prefix_length(&s.ordered_leaders, &new_snapshot.ordered_leaders)
                    == new_snapshot.ordered_leaders.len()
        }) {
            // Update existing snapshot
            tracing::debug!("Updating existing snapshot at position {}", pos);
            self.snapshots[pos] = new_snapshot;
        } else {
            // Add new snapshot
            tracing::debug!(
                "Adding new snapshot with {} leaders",
                new_snapshot.ordered_leaders.len()
            );
            self.snapshots.push(new_snapshot);

            // Optional: Limit snapshot count to prevent unbounded growth
            const MAX_SNAPSHOTS: usize = 100;
            if self.snapshots.len() > MAX_SNAPSHOTS {
                // Remove oldest snapshots (could use a more sophisticated eviction policy)
                self.snapshots.remove(0);
                tracing::debug!(
                    "Removed oldest snapshot, now maintaining {} snapshots",
                    self.snapshots.len()
                );
            }
        }
    }

    // async fn process_pending_blocks(&mut self) {
    //     while let Some(block) = self.pending_blocks.first() {
    //         // Extract transactions from the block
    //         let mut txs = Vec::<(String, Address)>::new();
    //         for statement in block.statements() {
    //             if let BaseStatement::Share(share) = statement {
    //                 let (raw_hex, caller) = decode_share_base_statement(share.data());
    //                 txs.push((raw_hex, caller));
    //             }
    //         }
    //         if txs.len() > 0 {
    //             tracing::info!("Executing {} transactions in pevm", txs.len());
    //             self.pevm_executor.execute(txs);
    //         }

    //         self.pending_blocks.remove(0);
    //     }
    // }
}
fn decode_share_base_statement(data: &[u8]) -> (String, Address) {
    let decoded: TransactionWithHint = bincode::deserialize(&data).unwrap();
    let raw_hex = decoded.raw_hex;
    let caller = decoded.caller;
    (raw_hex, caller)
}
