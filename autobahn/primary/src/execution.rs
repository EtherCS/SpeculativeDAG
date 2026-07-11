use anyhow::{anyhow, Result};
use config::Parameters;
use crypto::{Digest, PublicKey};
use ethers::types::Address;
use log::{debug, info, warn};
use pevm::api::{
    ensure_workload_artifacts, remove_invalid_workload_artifacts, ExecutionMode as PevmExecutionMode,
    EvmStateWriteSet, PevmExecutor, TransactionWithHint, WorkloadType,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use store::Store;
use tokio::sync::mpsc::Receiver;

use crate::Header;

#[derive(Clone, Debug)]
pub(crate) enum ExecutionRequest {
    Committed(Header),
    Proposed(Header),
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
    // Header id -> (committed-state version used as the speculative base, write set).
    speculative_results: HashMap<Digest, (u64, EvmStateWriteSet)>,
    committed_state_version: u64,
    pending_committed_headers: HashMap<PublicKey, BTreeMap<u64, Header>>,
    next_committed_height: HashMap<PublicKey, u64>,
}

impl ExecutionService {
    pub(crate) fn spawn(parameters: &Parameters, store: Store, rx_execution: Receiver<ExecutionRequest>) {
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
                pending_committed_headers: HashMap::new(),
                next_committed_height: HashMap::new(),
            }
            .run()
            .await;
        });
    }

    async fn run(&mut self) {
        while let Some(request) = self.rx_execution.recv().await {
            match request {
                ExecutionRequest::Committed(header) => {
                    self.buffer_committed_header(header).await;
                }
                ExecutionRequest::Proposed(header) => {
                    if self.config.strategy == ExecutionStrategy::SpeculativeAllCommit {
                        if let Err(error) = self.execute_speculative_header(&header).await {
                            warn!(
                                "Speculative EVM execution failed for header {} at height {}: {}",
                                header.id, header.height, error
                            );
                        }
                    }
                }
            }
        }
    }

    async fn buffer_committed_header(&mut self, header: Header) {
        let author = header.author;
        let height = header.height;
        self.pending_committed_headers
            .entry(author)
            .or_default()
            .insert(height, header);

        loop {
            let expected = *self.next_committed_height.entry(author).or_insert(1);
            let next_header = self
                .pending_committed_headers
                .get_mut(&author)
                .and_then(|headers| headers.remove(&expected));

            let Some(next_header) = next_header else {
                break;
            };

            match self.execute_committed_header(&next_header).await {
                Ok(()) => self.advance_committed_state(),
                Err(error) => {
                    warn!(
                        "Committed EVM execution failed for header {} at height {}: {}",
                        next_header.id, next_header.height, error
                    );
                }
            }

            self.next_committed_height.insert(author, expected + 1);
        }
    }

    async fn execute_committed_header(&mut self, header: &Header) -> Result<()> {
        if let Some((base_version, write_set)) = self.speculative_results.remove(&header.id) {
            if base_version == self.committed_state_version {
                info!(
                    "Committing speculative result for header {} without re-execution",
                    header.id
                );
                self.committed_executor
                    .commit_speculative_execution(write_set);
                self.log_execution_completion(header);
                return Ok(());
            }

            debug!(
                "Discarding stale speculative result for header {} (base version {}, current version {})",
                header.id,
                base_version,
                self.committed_state_version
            );
        }

        let txs = self.load_header_transactions(header).await?;
        if txs.is_empty() {
            return Ok(());
        }

        info!(
            "Executing committed header {} with {} EVM transactions",
            header.id,
            txs.len()
        );
        self.committed_executor.execute_checked(txs)?;
        self.log_execution_completion(header);
        Ok(())
    }

    fn advance_committed_state(&mut self) {
        self.committed_state_version += 1;
        let committed_state_version = self.committed_state_version;
        self.speculative_results
            .retain(|_, (base_version, _)| *base_version >= committed_state_version);
    }

    fn log_execution_completion(&self, header: &Header) {
        for digest in header.payload.keys() {
            // Emit this after the committed state transition is applied.
            info!("Executed {} -> {:?}", header, digest);
        }
    }

    async fn execute_speculative_header(&mut self, header: &Header) -> Result<()> {
        let txs = self.load_header_transactions(header).await?;

        let mut speculative_executor = self.committed_executor.clone();
        debug!(
            "Speculatively executing header {} with {} EVM transactions",
            header.id,
            txs.len()
        );
        let write_set = speculative_executor.speculative_execute(txs, EvmStateWriteSet::default());
        self.speculative_results.insert(
            header.id.clone(),
            (self.committed_state_version, write_set),
        );
        Ok(())
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
