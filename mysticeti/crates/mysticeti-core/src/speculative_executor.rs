use tokio::sync::mpsc;

pub use ethers::types::Address;
use pevm::api::{PevmExecutor, TransactionWithHint};

use crate::{
    consensus::linearizer::CommittedSubDag,
    data::Data,
    runtime::{self},
    types::{
        APSTree, BaseStatement, BlockReference, EvmStateWriteSet, SpeculativeExecutionSnapshot,
        StatementBlock,
    },
};

pub enum SpeculativeMessageStatus {
    Speculative, // This is a speculative order
    Consensus,   // This is the final consensus order
}
pub enum SpeculativeMessage {
    /// (speculative order, the corresponding sub_dags)
    SpeculativeExecuteTxs(APSTree, Vec<CommittedSubDag>, SpeculativeMessageStatus),
    CommitLeaders(Vec<Data<StatementBlock>>), // the ordered (to-commit) leader blocks via consensus
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
                        SpeculativeMessage::SpeculativeExecuteTxs(aps_tree, sub_dags, flag) => {
                            match flag {
                                SpeculativeMessageStatus::Speculative => {
                                    tracing::debug!("Received {} speculatively ordered blocks to execute", sub_dags.len());
                                    self.speculative_execution_on_blocks(&sub_dags).await;
                                },
                                SpeculativeMessageStatus::Consensus => {
                                    tracing::debug!("Received {} consensus ordered blocks to execute", sub_dags.len());
                                    // todo: update nodes reputations
                                    let committed_leaders: Vec<BlockReference> = sub_dags.iter().map(|sd| sd.anchor).collect();
                                    // Execute the committed blocks
                                    let new_states = self.speculative_execution_on_blocks(&sub_dags).await;
                                    // we can commit the new states as it is derived from consensus order
                                    self.pevm_executor.commit_speculative_execution(new_states);
                                    // Clean and update snapshots
                                    self.clean_and_update_snapshots(&committed_leaders);

                                },
                            }

                        },
                        SpeculativeMessage::CommitLeaders(ordered_leaders) => {
                            tracing::debug!("Received {} committed ordered blocks to execute", ordered_leaders.len());
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

    // perform speculative execution on the ordered blocks
    async fn speculative_execution_on_blocks(
        &mut self,
        sub_dags: &Vec<CommittedSubDag>,
    ) -> EvmStateWriteSet {
        // Find the best matching snapshot (longest common prefix with sub_dags)
        let best_snapshot = self.find_best_matching_snapshot(&sub_dags);

        match best_snapshot {
            Some(snapshot) => {
                tracing::debug!(
                    "Found matching snapshot with {} leaders, executing {} new leader blocks on top",
                    snapshot.ordered_leaders.len(),
                    sub_dags.len() - snapshot.ordered_leaders.len()
                );

                // Extract transactions from the new ordered blocks
                let mut new_ordered_leaders = vec![];
                let mut linearlized_new_blocks = vec![];
                for sub_dag in sub_dags[snapshot.ordered_leaders.len()..].iter() {
                    linearlized_new_blocks.extend(sub_dag.blocks.clone());
                    new_ordered_leaders.push(sub_dag.anchor);
                }

                let mut txs = Vec::<(String, Address)>::new();
                for block in &linearlized_new_blocks {
                    for statement in block.statements() {
                        if let BaseStatement::Share(share) = statement {
                            let (raw_hex, caller) = decode_share_base_statement(share.data());
                            txs.push((raw_hex, caller));
                        }
                    }
                }

                // Create new snapshot with combined order
                new_ordered_leaders.extend(snapshot.ordered_leaders.clone());

                // speculatively execute the new transactions on top of the snapshot state
                let snapshot_transition_states = snapshot.transition_states.clone();
                let new_state = self
                    .pevm_executor
                    .speculative_execute(txs, snapshot_transition_states);

                // todo: take snapshots based on reputation
                let new_snapshot =
                    SpeculativeExecutionSnapshot::new(new_ordered_leaders, new_state.clone());

                // Add the snapshot
                self.snapshots.push(new_snapshot);
                return new_state;
            }
            None => {
                tracing::debug!("No matching snapshot found, re-execute all transactions");
                // Extract transactions from the new ordered blocks
                let mut new_ordered_leaders = vec![];
                let mut linearlized_new_blocks = vec![];
                for sub_dag in sub_dags.iter() {
                    linearlized_new_blocks.extend(sub_dag.blocks.clone());
                    new_ordered_leaders.push(sub_dag.anchor);
                }

                let mut txs = Vec::<(String, Address)>::new();
                for block in &linearlized_new_blocks {
                    for statement in block.statements() {
                        if let BaseStatement::Share(share) = statement {
                            let (raw_hex, caller) = decode_share_base_statement(share.data());
                            txs.push((raw_hex, caller));
                        }
                    }
                }

                // execute the new transactions on top of the snapshot state
                let snapshot_transition_states = EvmStateWriteSet::default();
                let new_state = self
                    .pevm_executor
                    .speculative_execute(txs, snapshot_transition_states);

                // todo: take snapshots based on reputation
                let new_snapshot =
                    SpeculativeExecutionSnapshot::new(new_ordered_leaders, new_state.clone());

                // Add the snapshot
                self.snapshots.push(new_snapshot);

                return new_state;
            }
        }
    }

    /// Find the snapshot with the longest common prefix matching the given leaders
    fn find_best_matching_snapshot(
        &self,
        // target_leaders: &[Data<StatementBlock>],
        sub_dags: &Vec<CommittedSubDag>,
    ) -> Option<&SpeculativeExecutionSnapshot> {
        let mut best_match: Option<&SpeculativeExecutionSnapshot> = None;
        let mut best_match_length = 0;
        let mut target_leaders = vec![];
        for sub_dag in sub_dags {
            target_leaders.push(sub_dag.anchor);
        }

        for snapshot in self.snapshots.iter().rev() {
            let common_prefix_len =
                common_prefix_length(&snapshot.ordered_leaders, &target_leaders);

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

    /// Clean and update snapshots after a set of leaders has been committed
    fn clean_and_update_snapshots(&mut self, committed_leaders: &[BlockReference]) {
        if committed_leaders.is_empty() {
            return;
        }

        let mut updated = Vec::with_capacity(self.snapshots.len());

        for mut snapshot in self.snapshots.drain(..) {
            let prefix_len = common_prefix_length(&snapshot.ordered_leaders, committed_leaders);

            if prefix_len == committed_leaders.len() {
                // The committed leaders are a full prefix of this snapshot; drop that prefix
                snapshot.ordered_leaders.drain(..prefix_len);
                updated.push(snapshot);
            } else {
                // Prefix is inconsistent with the commit; discard this snapshot
                tracing::debug!(
                    "Dropping snapshot with fully committed leaders or with inconsistent prefix (snapshot_len={}, common_prefix_len={}, committed_len={})",
                    snapshot.ordered_leaders.len(),
                    prefix_len,
                    committed_leaders.len()
                );
            }
        }

        self.snapshots = updated;
    }
}

fn decode_share_base_statement(data: &[u8]) -> (String, Address) {
    let decoded: TransactionWithHint = bincode::deserialize(&data).unwrap();
    let raw_hex = decoded.raw_hex;
    let caller = decoded.caller;
    (raw_hex, caller)
}

/// Calculate the length of the common prefix between two leader sequences
fn common_prefix_length(leaders_a: &[BlockReference], leaders_b: &[BlockReference]) -> usize {
    leaders_a
        .iter()
        .zip(leaders_b.iter())
        .take_while(|(a, b)| a == b)
        .count()
}
