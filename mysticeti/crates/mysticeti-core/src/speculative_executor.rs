use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub use ethers::types::Address;
use pevm::api::{PevmExecutor, TransactionWithHint};

use crate::{
    consensus::linearizer::CommittedSubDag,
    data::Data,
    metrics::Metrics,
    node_reputation::NodeReputation,
    runtime::{self},
    transactions_generator::TransactionGenerator,
    types::{
        APSTree, BaseStatement, BlockReference, EvmStateWriteSet, LeaderPredictionStatus,
        SpeculativeExecutionSnapshot, StatementBlock,
    },
};

const SNAPSHOT_WINDOW: usize = 10; // the maximum number of temporary snapshots

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
    pub pevm_executor: Arc<Mutex<PevmExecutor>>,
    /// The receiver of speculatively ordered blocks from core
    pub speculative_message_receiver: mpsc::Receiver<SpeculativeMessage>,
    /// The maintained speculative execution snapshots
    pub snapshots: Vec<SpeculativeExecutionSnapshot>,
    /// The sliding window of recent snapshots for commitment
    /// The last element will serve as the last executed speculative snapshot
    pub snapshot_window: VecDeque<SpeculativeExecutionSnapshot>,
    /// The node reputation tracker
    pub node_reputation: NodeReputation,
    metrics: Arc<Metrics>,
}

impl SpeculativeExecutor {
    pub fn start(
        pevm_executor: PevmExecutor,
        speculative_message_receiver: mpsc::Receiver<SpeculativeMessage>,
        node_reputation: NodeReputation,
        metrics: Arc<Metrics>,
    ) {
        runtime::Handle::current().spawn(async move {
            // Create an initial empty snapshot as the base state
            let initial_snapshot = SpeculativeExecutionSnapshot::new(
                Vec::new(),                  // No leaders yet
                EvmStateWriteSet::default(), // Empty initial state
            );

            Self {
                pevm_executor: Arc::new(Mutex::new(pevm_executor)),
                speculative_message_receiver,
                snapshots: vec![initial_snapshot.clone()], // Start with base snapshot
                snapshot_window: VecDeque::with_capacity(SNAPSHOT_WINDOW),
                node_reputation,
                metrics,
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
                                    tracing::debug!("Received {} speculatively ordered blocks to execute, leaders {:?}", sub_dags.len(), sub_dags.iter().map(|sd| sd.anchor).collect::<Vec<BlockReference>>());
                                    let now_time = std::time::Instant::now();

                                    self.speculative_execution_on_blocks(&sub_dags).await;

                                    let elapsed = now_time.elapsed();
                                    tracing::debug!("Speculative execution of {:?} blocks took {:?}", sub_dags.iter().map(|sd| sd.anchor).collect::<Vec<BlockReference>>(), elapsed);
                                },
                                SpeculativeMessageStatus::Consensus => {
                                    tracing::debug!("Received {} consensus ordered blocks {:?} to execute", sub_dags.len(), sub_dags.iter().map(|sd| sd.anchor).collect::<Vec<BlockReference>>());
                                    let start_time = std::time::Instant::now();

                                    // for the consensus message, sub_dags is the newly committed anchors
                                    let committed_leaders: Vec<BlockReference> = sub_dags.iter().map(|sd| sd.anchor).collect();

                                    // Execute the committed blocks (based on the speculative execution snapshots)
                                    let new_states = self.consensus_execution_on_blocks(&sub_dags).await;

                                    let elapsed = start_time.elapsed();
                                    tracing::debug!("Consensus execution of {} blocks took {:?}", sub_dags.len(), elapsed);

                                     // Commit with mutex lock in blocking context
                                    let pevm_executor = Arc::clone(&self.pevm_executor);
                                    tokio::task::spawn_blocking(move || {
                                        let mut executor = pevm_executor.lock().unwrap();
                                        executor.commit_speculative_execution(new_states);
                                    }).await.expect("Commit task failed");

                                    // record the end-to-end tx latency
                                    let current_timestamp = runtime::timestamp_utc();
                                    let metrics = self.metrics.clone();
                                    tokio::task::spawn_blocking(move || {
                                        for sub_dag in sub_dags.iter() {
                                            for block in &sub_dag.blocks {
                                                for statement in block.statements() {
                                                    if let BaseStatement::Share(share) = statement {
                                                        let creation_time = TransactionGenerator::extract_timestamp(share);
                                                        metrics
                                                            .transaction_committed_latency
                                                            .observe(current_timestamp.saturating_sub(creation_time));
                                                    }
                                                }
                                            }
                                        }
                                    }).await.expect("Latency recording task failed");

                                    // update node reputations based on the committed leaders and the previous predictions
                                    let committed_leaders_rounds = committed_leaders.iter().map(|b| b.round).collect::<Vec<u64>>();
                                    let last_committed_leader_round = *committed_leaders_rounds.last().unwrap_or(&0);
                                    for predicted in aps_tree.pending_leaders.iter().take_while(|p| p.round <= last_committed_leader_round) {
                                        let hit = committed_leaders_rounds.contains(&predicted.round);
                                        match (predicted.status, hit) {
                                            (LeaderPredictionStatus::Committed, true) => self.node_reputation.update_score(predicted.author, 1),  // correct
                                            (LeaderPredictionStatus::Committed, false) => self.node_reputation.update_score(predicted.author, -1), // incorrect
                                            (LeaderPredictionStatus::Skipped, true) => self.node_reputation.update_score(predicted.author, -1),    // missed
                                            (LeaderPredictionStatus::Skipped, false) => self.node_reputation.update_score(predicted.author, 1),    // correct skip
                                        }
                                    }

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
                // Todo: We might need to handle error here
            }
        }
    }

    /// perform speculative execution on the ordered blocks
    /// take snapshots based on the node reputation
    async fn speculative_execution_on_blocks(&mut self, sub_dags: &Vec<CommittedSubDag>) {
        // execute new sub dags based on the current
        let last_executed_snapshot = self.snapshot_window.back().cloned().unwrap_or_else(|| {
            SpeculativeExecutionSnapshot::new(
                Vec::new(),                  // No leaders yet
                EvmStateWriteSet::default(), // Empty initial state
            )
        });
        let mut new_state = last_executed_snapshot.transition_states.clone();
        let mut new_ordered_leaders = last_executed_snapshot.ordered_leaders.clone();

        // Clone executor Arc once outside the loop
        let executor = Arc::clone(&self.pevm_executor);

        for (idx, sub_dag) in sub_dags.iter().enumerate() {
            if idx > 0 && idx % 5 == 0 {
                tokio::task::yield_now().await;
            }

            if self.node_reputation.get_score(sub_dag.anchor.authority)
                < self.node_reputation.threshold_score()
            {
                // we take snapshot if the next leader is likely malicious
                self.snapshots.push(SpeculativeExecutionSnapshot::new(
                    new_ordered_leaders.clone(),
                    new_state.clone(),
                ));
            }

            let mut txs = Vec::<(String, Address)>::new();
            let block_count = sub_dag.blocks.len();

            for block in &sub_dag.blocks {
                for statement in block.statements() {
                    if let BaseStatement::Share(share) = statement {
                        let (raw_hex, caller) = decode_share_base_statement(share.data());
                        txs.push((raw_hex, caller));
                    }
                }
            }

            let start_execution_time = std::time::Instant::now();

            // Execute in spawn_blocking to avoid blocking the async runtime
            let exec_ref = Arc::clone(&executor);
            new_state = tokio::task::spawn_blocking(move || {
                let mut exec = exec_ref.lock().unwrap();
                exec.speculative_execute(txs, new_state)
            })
            .await
            .expect("Execution task failed");

            let end_execution_time = start_execution_time.elapsed();

            // Record the average block execution time
            if block_count > 0 {
                self.metrics
                    .block_execution_latency
                    .observe(end_execution_time / block_count as u32);
            }

            new_ordered_leaders.push(sub_dag.anchor);

            // we take snapshot after speculative execution if the next leader is likely correct
            if self.snapshot_window.len() >= SNAPSHOT_WINDOW {
                self.snapshot_window.pop_front();
            }
            self.snapshot_window
                .push_back(SpeculativeExecutionSnapshot::new(
                    new_ordered_leaders.clone(),
                    new_state.clone(),
                ));
        }
    }

    /// perform consensus execution on the committed ordered blocks
    /// Note: the return states are the committed states after executing all the committed blocks
    async fn consensus_execution_on_blocks(
        &mut self,
        sub_dags: &Vec<CommittedSubDag>,
    ) -> EvmStateWriteSet {
        // Find the best matching snapshot (longest common prefix with sub_dags)
        let best_snapshot = self.find_best_matching_snapshot(&sub_dags);

        match best_snapshot {
            Some(snapshot) => {
                let hit_count = snapshot.ordered_leaders.len();
                tracing::debug!(
                    "Consensus execution: found matching snapshot with {} leaders, executing {} new leader blocks on top",
                    hit_count,
                    sub_dags.len() - hit_count
                );
                let mut new_state = snapshot.transition_states.clone();

                // Clone executor Arc once outside the loop
                let executor = Arc::clone(&self.pevm_executor);

                for (idx, sub_dag) in sub_dags[hit_count..].iter().enumerate() {
                    // Yield to runtime every few iterations to prevent blocking
                    if idx > 0 && idx % 5 == 0 {
                        tokio::task::yield_now().await;
                    }

                    let mut txs = Vec::<(String, Address)>::new();
                    let block_count = sub_dag.blocks.len();

                    for block in &sub_dag.blocks {
                        for statement in block.statements() {
                            if let BaseStatement::Share(share) = statement {
                                let (raw_hex, caller) = decode_share_base_statement(share.data());
                                txs.push((raw_hex, caller));
                            }
                        }
                    }

                    let start_execution_time = std::time::Instant::now();

                    // Execute in spawn_blocking to avoid blocking the async runtime
                    let exec_ref = Arc::clone(&executor);
                    new_state = tokio::task::spawn_blocking(move || {
                        let mut exec = exec_ref.lock().unwrap();
                        exec.speculative_execute(txs, new_state)
                    })
                    .await
                    .expect("Execution task failed");

                    let end_execution_time = start_execution_time.elapsed();

                    // Record the average block execution time
                    if block_count > 0 {
                        self.metrics
                            .block_execution_latency
                            .observe(end_execution_time / block_count as u32);
                    }
                }

                new_state
            }
            None => {
                tracing::debug!(
                    "Consensus execution: no matching snapshot found, execute {} leader blocks",
                    sub_dags.len()
                );
                let mut new_state = EvmStateWriteSet::default();

                // Clone executor Arc once outside the loop
                let executor = Arc::clone(&self.pevm_executor);

                for (idx, sub_dag) in sub_dags.iter().enumerate() {
                    // Yield to runtime every few iterations to prevent blocking
                    if idx > 0 && idx % 5 == 0 {
                        tokio::task::yield_now().await;
                    }

                    let mut txs = Vec::<(String, Address)>::new();
                    let block_count = sub_dag.blocks.len();

                    for block in &sub_dag.blocks {
                        for statement in block.statements() {
                            if let BaseStatement::Share(share) = statement {
                                let (raw_hex, caller) = decode_share_base_statement(share.data());
                                txs.push((raw_hex, caller));
                            }
                        }
                    }

                    let start_execution_time = std::time::Instant::now();

                    // Execute in spawn_blocking to avoid blocking the async runtime
                    let exec_ref = Arc::clone(&executor);
                    new_state = tokio::task::spawn_blocking(move || {
                        let mut exec = exec_ref.lock().unwrap();
                        exec.speculative_execute(txs, new_state)
                    })
                    .await
                    .expect("Execution task failed");

                    let end_execution_time = start_execution_time.elapsed();

                    // Record the average block execution time
                    if block_count > 0 {
                        self.metrics
                            .block_execution_latency
                            .observe(end_execution_time / block_count as u32);
                    }
                }

                new_state
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

        // we should consider the recent snapshots in self.snapshot_window to prevent state overwritten issue
        let all_snapshots: Vec<&SpeculativeExecutionSnapshot> = self
            .snapshots
            .iter()
            .chain(self.snapshot_window.iter())
            .collect();

        for snapshot in all_snapshots.iter().rev() {
            let common_prefix_len =
                common_prefix_length(&snapshot.ordered_leaders, &target_leaders);

            if common_prefix_len == target_leaders.len()
                && common_prefix_len == snapshot.ordered_leaders.len()
            {
                // Perfect match
                return Some(snapshot);
            }
            if common_prefix_len > best_match_length {
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

        // Also update the snapshot window
        let mut updated_window = VecDeque::with_capacity(self.snapshot_window.len());
        for mut snapshot in self.snapshot_window.drain(..) {
            let prefix_len = common_prefix_length(&snapshot.ordered_leaders, committed_leaders);
            if prefix_len == committed_leaders.len() {
                // The committed leaders are a full prefix of this snapshot; drop that prefix
                snapshot.ordered_leaders.drain(..prefix_len);
                updated_window.push_back(snapshot);
            } else {
                // Prefix is inconsistent with the commit; discard this snapshot
                tracing::debug!(
                    "Dropping snapshot in window with fully committed leaders or with inconsistent prefix (snapshot_len={}, common_prefix_len={}, committed_len={})",
                    snapshot.ordered_leaders.len(),
                    prefix_len,
                    committed_leaders.len()
                );
            }
        }
        self.snapshot_window = updated_window;
    }
}

/// Decode a share base statement to extract the transaction creation time, raw hex, and caller address
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
