use tokio::sync::mpsc;

pub use ethers::types::Address;
use pevm::api::{PevmExecutor, TransactionWithHint};

use crate::{
    consensus::linearizer::CommittedSubDag,
    data::Data,
    node_reputation::NodeReputation,
    runtime::{self},
    types::{
        APSTree, BaseStatement, BlockReference, EvmStateWriteSet, LeaderPredictionStatus,
        SpeculativeExecutionSnapshot, StatementBlock,
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
    /// The node reputation tracker
    pub node_reputation: NodeReputation,
}

impl SpeculativeExecutor {
    pub fn start(
        pevm_executor: PevmExecutor,
        speculative_message_receiver: mpsc::Receiver<SpeculativeMessage>,
        node_reputation: NodeReputation,
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
                node_reputation,
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
                                    tracing::debug!("Received {} consensus ordered blocks {:?} to execute", sub_dags.len(), sub_dags.iter().map(|sd| sd.anchor).collect::<Vec<BlockReference>>());

                                    let committed_leaders: Vec<BlockReference> = sub_dags.iter().map(|sd| sd.anchor).collect();

                                    // update node reputations based on the committed leaders and the previous predictions
                                    let committed_leaders_rounds = committed_leaders.iter().map(|b| b.round).collect::<Vec<u64>>();
                                    let last_committed_leader_round = committed_leaders_rounds.last().cloned().unwrap_or(0);

                                    for predicted in aps_tree.pending_leaders.iter().take_while(|p| p.round <= last_committed_leader_round) {
                                        let hit = committed_leaders_rounds.contains(&predicted.round);
                                        match (predicted.clone().status, hit) {
                                            (LeaderPredictionStatus::Committed, true) => self.node_reputation.update_score(predicted.author, 1),  // correct
                                            (LeaderPredictionStatus::Committed, false) => self.node_reputation.update_score(predicted.author, -1), // incorrect
                                            (LeaderPredictionStatus::Skipped, true) => self.node_reputation.update_score(predicted.author, -1),    // missed
                                            (LeaderPredictionStatus::Skipped, false) => self.node_reputation.update_score(predicted.author, 1),    // correct skip
                                        }
                                    }

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
                let mut new_ordered_leaders = snapshot.ordered_leaders.clone();
                let mut new_state = snapshot.transition_states.clone();
                for sub_dag in sub_dags[snapshot.ordered_leaders.len()..].iter() {
                    let mut txs = Vec::<(String, Address)>::new();
                    for block in &sub_dag.blocks {
                        for statement in block.statements() {
                            if let BaseStatement::Share(share) = statement {
                                let (raw_hex, caller) = decode_share_base_statement(share.data());
                                txs.push((raw_hex, caller));
                            }
                        }
                    }
                    new_state = self
                        .pevm_executor
                        .speculative_execute(txs, new_state.clone());

                    new_ordered_leaders.push(sub_dag.anchor);
                    if self.node_reputation.get_score(sub_dag.anchor.authority)
                        >= self.node_reputation.threshold_score()
                    {
                        tracing::debug!(
                            "Leader {:?} has high reputation score {}, above threshold {}, take a snapshot",
                            sub_dag.anchor,
                            self.node_reputation.get_score(sub_dag.anchor.authority),
                            self.node_reputation.threshold_score()
                        );
                        // we take snapshot after speculative execution if the next leader is likely correct
                        self.snapshots.push(SpeculativeExecutionSnapshot::new(
                            new_ordered_leaders.clone(),
                            new_state.clone(),
                        ));
                    }
                }

                return new_state;
            }
            None => {
                tracing::debug!("No matching snapshot found, re-execute all transactions");
                // Extract transactions from the new ordered blocks
                let mut new_ordered_leaders = vec![];
                let mut new_state = EvmStateWriteSet::default();
                for sub_dag in sub_dags.iter() {
                    let mut txs = Vec::<(String, Address)>::new();
                    for block in &sub_dag.blocks {
                        for statement in block.statements() {
                            if let BaseStatement::Share(share) = statement {
                                let (raw_hex, caller) = decode_share_base_statement(share.data());
                                txs.push((raw_hex, caller));
                            }
                        }
                    }
                    new_state = self
                        .pevm_executor
                        .speculative_execute(txs, new_state.clone());

                    new_ordered_leaders.push(sub_dag.anchor);
                    if self.node_reputation.get_score(sub_dag.anchor.authority)
                        >= self.node_reputation.threshold_score()
                    {
                        tracing::debug!(
                            "Leader {:?} has high reputation score {}, above threshold {}, take a snapshot",
                            sub_dag.anchor,
                            self.node_reputation.get_score(sub_dag.anchor.authority),
                            self.node_reputation.threshold_score()
                        );
                        // we take snapshot after speculative execution if the next leader is likely correct
                        self.snapshots.push(SpeculativeExecutionSnapshot::new(
                            new_ordered_leaders.clone(),
                            new_state.clone(),
                        ));
                    }
                }

                return new_state;
            }
        }
    }

    /// Find the snapshot with the longest common prefix matching the given leaders
    fn find_best_matching_snapshot(
        &self,
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

            if common_prefix_len == target_leaders.len() && common_prefix_len == snapshot.ordered_leaders.len() {
                // Perfect match
                return Some(snapshot);
            }
            if common_prefix_len > best_match_length
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
