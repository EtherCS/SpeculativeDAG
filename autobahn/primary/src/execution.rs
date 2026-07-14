use anyhow::{anyhow, Result};
use config::Parameters;
use crypto::{Digest, PublicKey};
use ethers::types::Address;
use log::{debug, info, warn};
use pevm::api::{
    ensure_workload_artifacts, remove_invalid_workload_artifacts, EvmStateWriteSet,
    ExecutionMode as PevmExecutionMode, PevmExecutor, TransactionWithHint, WorkloadType,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use store::Store;
use tokio::sync::mpsc::Receiver;

use crate::messages::{ConsensusMessage, Proposal};
use crate::Header;
use crate::primary::{Slot, View};

#[derive(Clone, Debug)]
pub(crate) enum ExecutionRequest {
    Committed(ConsensusMessage),
    Proposed(ConsensusMessage),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ProposalKey {
    slot: Slot,
    view: View,
    // HashMap iteration order is randomized, so proposal identity must be canonical.
    proposals: Vec<(PublicKey, u64, Digest)>,
}

impl ProposalKey {
    fn from_message(message: &ConsensusMessage) -> Option<Self> {
        let (slot, view, proposals) = match message {
            ConsensusMessage::Prepare {
                slot,
                view,
                proposals,
                ..
            }
            | ConsensusMessage::Commit {
                slot,
                view,
                proposals,
                ..
            } => (*slot, *view, proposals),
            ConsensusMessage::Confirm { .. } => return None,
        };

        let mut proposals = proposals
            .iter()
            .map(|(author, proposal)| (*author, proposal.height, proposal.header_digest.clone()))
            .collect::<Vec<_>>();
        proposals.sort_by_key(|(author, _, _)| *author);

        Some(Self {
            slot,
            view,
            proposals,
        })
    }
}

#[derive(Debug)]
struct SpeculativeProposalResult {
    base_version: u64,
    header_ids: Vec<Digest>,
    write_set: EvmStateWriteSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ExecutionStrategy {
    AfterOrdering,
    SpeculativeAllCommit,
}

#[derive(Clone, Debug)]
struct ExecutionConfig {
    strategy: ExecutionStrategy,
    workload: WorkloadType,
    executor_mode: PevmExecutionMode,
    account_storage_path: String,
    account_addresses_path: String,
}

impl ExecutionConfig {
    fn from_parameters(parameters: &Parameters) -> Result<Option<Self>> {
        let strategy = match parameters.evm_execution_mode.to_ascii_lowercase().as_str() {
            "none" | "disabled" => return Ok(None),
            "ordered" | "after-ordering" | "execution-after-ordering" => {
                ExecutionStrategy::AfterOrdering
            }
            "speculative" | "speculative-execution" | "all-commit" => {
                ExecutionStrategy::SpeculativeAllCommit
            }
            other => return Err(anyhow!("unsupported EVM execution mode '{other}'")),
        };

        let workload = match parameters.evm_workload.to_ascii_lowercase().as_str() {
            "erc20" => WorkloadType::ERC20(
                parameters.evm_num_clusters,
                parameters.evm_num_families_per_cluster,
                parameters.evm_num_people_per_family,
            ),
            "weth" => WorkloadType::WETH(
                parameters.evm_num_clusters,
                parameters.evm_num_families_per_cluster,
                parameters.evm_num_people_per_family,
            ),
            "uniswap" => WorkloadType::Uniswap(
                parameters.evm_num_clusters,
                parameters.evm_num_families_per_cluster,
                parameters.evm_num_people_per_family,
            ),
            other => return Err(anyhow!("unsupported workload '{other}'")),
        };

        let executor_mode = match parameters.evm_executor_mode.to_ascii_lowercase().as_str() {
            "sequential" => PevmExecutionMode::Sequential,
            "parallel" => PevmExecutionMode::Parallel,
            other => return Err(anyhow!("unsupported PEVM executor mode '{other}'")),
        };

        let artifacts_dir = if parameters.evm_artifacts_dir.is_empty() {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf()
        } else {
            PathBuf::from(&parameters.evm_artifacts_dir)
        };
        let (storage_file, addresses_file) = workload.artifact_file_names();
        let account_storage_path = artifacts_dir.join(storage_file);
        let account_addresses_path = artifacts_dir.join(addresses_file);

        Ok(Some(Self {
            strategy,
            workload,
            executor_mode,
            account_storage_path: account_storage_path.to_string_lossy().into_owned(),
            account_addresses_path: account_addresses_path.to_string_lossy().into_owned(),
        }))
    }
}

#[derive(Debug, Deserialize)]
enum StoredWorkerMessage {
    Batch(Vec<Vec<u8>>),
    BatchRequest(Vec<Digest>, PublicKey),
}

pub(crate) struct ExecutionService {
    config: ExecutionConfig,
    store: Store,
    rx_execution: Receiver<ExecutionRequest>,
    committed_executor: PevmExecutor,
    // Consensus proposal -> result for its complete, ordered header sequence.
    speculative_results: HashMap<ProposalKey, SpeculativeProposalResult>,
    committed_state_version: u64,
    committed_heights: HashMap<PublicKey, u64>,
}

impl ExecutionService {
    pub(crate) fn spawn(
        parameters: &Parameters,
        store: Store,
        rx_execution: Receiver<ExecutionRequest>,
    ) {
        let config = match ExecutionConfig::from_parameters(parameters) {
            Ok(Some(config)) => config,
            Ok(None) => return,
            Err(error) => {
                warn!("Skipping EVM execution initialization: {error}");
                return;
            }
        };

        remove_invalid_workload_artifacts(
            &config.account_storage_path,
            &config.account_addresses_path,
        );
        if let Err(error) = ensure_workload_artifacts(
            &config.workload,
            &config.account_storage_path,
            &config.account_addresses_path,
        ) {
            warn!("Failed to prepare PEVM workload artifacts: {error}");
            return;
        }

        let committed_executor = PevmExecutor::new(
            config.executor_mode.clone(),
            config.workload.clone(),
            config.account_storage_path.clone(),
        );
        info!(
            "Starting EVM execution service in {:?} mode for {:?}",
            config.strategy, config.workload
        );

        tokio::spawn(async move {
            Self {
                config,
                store,
                rx_execution,
                committed_executor,
                speculative_results: HashMap::new(),
                committed_state_version: 0,
                committed_heights: HashMap::new(),
            }
            .run()
            .await;
        });
    }

    async fn run(&mut self) {
        while let Some(request) = self.rx_execution.recv().await {
            match request {
                ExecutionRequest::Committed(message) => {
                    if let Err(error) = self.execute_committed_proposal(&message).await {
                        warn!("Committed EVM execution failed: {:?}", error);
                    }
                }
                ExecutionRequest::Proposed(message) => {
                    if self.config.strategy == ExecutionStrategy::SpeculativeAllCommit {
                        if let Err(error) = self.execute_speculative_proposal(&message).await {
                            warn!("Speculative EVM execution failed for Prepare: {:?}", error);
                        }
                    }
                }
            }
        }
    }

    async fn execute_committed_proposal(&mut self, message: &ConsensusMessage) -> Result<()> {
        let key = ProposalKey::from_message(message)
            .ok_or_else(|| anyhow!("execution received a non-Commit consensus message"))?;
        let headers = self.load_proposal_headers(message).await?;
        let header_ids = headers.iter().map(|header| header.id.clone()).collect::<Vec<_>>();

        let mut reused = false;
        if let Some(result) = self.speculative_results.remove(&key) {
            if result.base_version == self.committed_state_version
                && result.header_ids == header_ids
            {
                info!(
                    "Committing speculative result for Prepare slot {} view {} ({} headers) without re-execution",
                    key.slot,
                    key.view,
                    headers.len()
                );
                self.committed_executor
                    .commit_speculative_execution(result.write_set);
                reused = true;
            } else {
                debug!(
                    "Discarding stale or mismatched speculative result for slot {} view {} (base version {}, current version {})",
                    key.slot,
                    key.view,
                    result.base_version,
                    self.committed_state_version
                );
            }
        }

        if !reused {
            let txs = self.load_headers_transactions(&headers).await?;
            if !txs.is_empty() {
                info!(
                    "Executing committed Prepare slot {} view {} with {} headers and {} EVM transactions",
                    key.slot,
                    key.view,
                    headers.len(),
                    txs.len()
                );
                self.committed_executor.execute_checked(txs)?;
            }
        }

        for header in &headers {
            self.log_execution_completion(header);
            self.committed_heights
                .entry(header.author)
                .and_modify(|height| *height = (*height).max(header.height))
                .or_insert(header.height);
        }
        self.advance_committed_state(key.slot);
        Ok(())
    }

    fn advance_committed_state(&mut self, committed_slot: Slot) {
        self.committed_state_version += 1;
        let committed_state_version = self.committed_state_version;
        self.speculative_results.retain(|key, result| {
            key.slot > committed_slot && result.base_version >= committed_state_version
        });
    }

    fn log_execution_completion(&self, header: &Header) {
        for digest in header.payload.keys() {
            // Emit this after the committed state transition is applied.
            info!("Executed {} -> {:?}", header, digest);
        }
    }

    async fn execute_speculative_proposal(&mut self, message: &ConsensusMessage) -> Result<()> {
        let key = ProposalKey::from_message(message)
            .ok_or_else(|| anyhow!("speculation received a non-Prepare consensus message"))?;
        if self.speculative_results.contains_key(&key) {
            return Ok(());
        }

        let headers = self.load_proposal_headers(message).await?;
        let txs = self.load_headers_transactions(&headers).await?;

        let mut speculative_executor = self.committed_executor.clone();
        debug!(
            "Speculatively executing Prepare slot {} view {} with {} headers and {} EVM transactions",
            key.slot,
            key.view,
            headers.len(),
            txs.len()
        );
        let write_set = speculative_executor.speculative_execute(txs, EvmStateWriteSet::default());
        self.speculative_results.insert(
            key,
            SpeculativeProposalResult {
                base_version: self.committed_state_version,
                header_ids: headers.iter().map(|header| header.id.clone()).collect(),
                write_set,
            },
        );
        Ok(())
    }

    async fn load_proposal_headers(
        &mut self,
        message: &ConsensusMessage,
    ) -> Result<Vec<Header>> {
        let proposals = match message {
            ConsensusMessage::Prepare { proposals, .. }
            | ConsensusMessage::Commit { proposals, .. } => proposals,
            ConsensusMessage::Confirm { .. } => {
                return Err(anyhow!("cannot execute a Confirm message"));
            }
        };

        let mut proposals = proposals.iter().collect::<Vec<_>>();
        proposals.sort_by_key(|(author, _)| **author);

        let mut ordered = Vec::new();
        for (author, proposal) in proposals {
            let stop_height = self.committed_heights.get(author).copied().unwrap_or(0);
            if proposal.height <= stop_height {
                continue;
            }
            ordered.extend(self.load_proposal_chain(proposal, stop_height).await?);
        }
        Ok(ordered)
    }

    async fn load_proposal_chain(
        &mut self,
        proposal: &Proposal,
        stop_height: u64,
    ) -> Result<Vec<Header>> {
        let mut headers = Vec::new();
        let mut digest = proposal.header_digest.clone();
        let mut height = proposal.height;

        while height > stop_height {
            let bytes = self
                .store
                .read(digest.to_vec())
                .await?
                .ok_or_else(|| anyhow!("missing header {} at height {}", digest, height))?;
            let header: Header = bincode::deserialize(&bytes)?;
            digest = header.parent_cert.header_digest.clone();
            height = header.parent_cert.height;
            headers.push(header);
        }
        // Proposal tips point backwards. EVM transactions must execute from the
        // oldest uncommitted header to the newest to preserve account nonces.
        headers.reverse();
        Ok(headers)
    }

    async fn load_headers_transactions(
        &mut self,
        headers: &[Header],
    ) -> Result<Vec<(String, Address)>> {
        let mut txs = Vec::new();
        for header in headers {
            txs.extend(self.load_header_transactions(header).await?);
        }
        Ok(txs)
    }

    async fn load_header_transactions(
        &mut self,
        header: &Header,
    ) -> Result<Vec<(String, Address)>> {
        let mut ordered = Vec::new();
        for digest in header.payload.keys() {
            let bytes = self
                .store
                .read(digest.to_vec())
                .await?
                .ok_or_else(|| anyhow!("missing batch for digest {}", digest))?;
            ordered.extend(Self::decode_batch(&bytes)?);
        }
        ordered.sort_by_key(|txn| txn.timestamp);
        Ok(ordered
            .into_iter()
            .map(|txn| (txn.raw_hex, txn.caller))
            .collect())
    }

    fn decode_batch(bytes: &[u8]) -> Result<Vec<TransactionWithHint>> {
        match bincode::deserialize::<StoredWorkerMessage>(bytes)? {
            StoredWorkerMessage::Batch(batch) => batch
                .into_iter()
                .map(|bytes| Ok(bincode::deserialize::<TransactionWithHint>(&bytes)?))
                .collect(),
            StoredWorkerMessage::BatchRequest(_, _) => Ok(Vec::new()),
        }
    }
}
