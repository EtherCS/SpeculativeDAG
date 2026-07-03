// Copyright(C) Facebook, Inc. and its affiliates.
use anyhow::{Context, Result};
use bytes::BufMut as _;
use bytes::BytesMut;
use clap::{crate_name, crate_version, App, AppSettings};
use env_logger::Env;
use futures::future::join_all;
use futures::sink::SinkExt as _;
use log::{info, warn};
use pevm::api::{
    ensure_workload_artifacts, PevmTransactionGenerator, TransactionWithHint, WorkloadType,
};
use rand::Rng;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep, Duration, Instant};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[tokio::main]
async fn main() -> Result<()> {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .about("Benchmark client for Sailfish.")
        .args_from_usage("<ADDR> 'The network address of the node where to send txs'")
        .args_from_usage("--size=<INT> 'The size of each transaction in bytes'")
        .args_from_usage("--rate=<INT> 'The rate (txs/s) at which to send the transactions'")
        .args_from_usage("--nodes=[ADDR]... 'Network addresses that must be reachable before starting the benchmark.'")
        .args_from_usage("--workload=[NAME] 'PEVM workload: erc20|weth|uniswap'")
        .args_from_usage("--artifacts=[PATH] 'Directory for PEVM workload artifacts'")
        .args_from_usage("--num-clusters=[INT] 'Number of workload clusters'")
        .args_from_usage("--families-per-cluster=[INT] 'Number of families per cluster'")
        .args_from_usage("--people-per-family=[INT] 'Number of people per family'")
        .args_from_usage("--replica-id=[INT] 'Replica id used by the PEVM transaction generator'")
        .args_from_usage("--replica-num=[INT] 'Replica count used by the PEVM transaction generator'")
        .setting(AppSettings::ArgRequiredElseHelp)
        .get_matches();

    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let target = matches
        .value_of("ADDR")
        .unwrap()
        .parse::<SocketAddr>()
        .context("Invalid socket address format")?;
    let size = matches
        .value_of("size")
        .unwrap()
        .parse::<usize>()
        .context("The size of transactions must be a non-negative integer")?;
    let rate = matches
        .value_of("rate")
        .unwrap()
        .parse::<u64>()
        .context("The rate of transactions must be a non-negative integer")?;
    let nodes = matches
        .values_of("nodes")
        .unwrap_or_default()
        .into_iter()
        .map(|x| x.parse::<SocketAddr>())
        .collect::<Result<Vec<_>, _>>()
        .context("Invalid socket address format")?;

    info!("Node address: {}", target);

    // NOTE: This log entry is used to compute performance.
    info!("Transactions size: {} B", size);

    // NOTE: This log entry is used to compute performance.
    info!("Transactions rate: {} tx/s", rate);

    let client = Client {
        target,
        size,
        rate,
        nodes,
        evm: EvmClientConfig::from_matches(&matches)?,
    };

    // Wait for all nodes to be online and synchronized.
    client.wait().await;

    // Start the benchmark.
    client.send().await.context("Failed to submit transactions")
}

struct Client {
    target: SocketAddr, //specifies the worker to connect to
    size: usize,        //specifies the bit size of transactions
    rate: u64,
    nodes: Vec<SocketAddr>, //specifies the addresses of all nodes. Currently only used to wait for them to be alive, but also necessary if we wanted to receive result replies (from any node).
    evm: Option<EvmClientConfig>,
}

impl Client {
    pub async fn send(&self) -> Result<()> {
        const PRECISION: u64 = 20; // Sample precision.
        const BURST_DURATION: u64 = 1000 / PRECISION;

        // The transaction size must be at least 16 bytes to ensure all txs are different.
        if self.evm.is_none() && self.size < 9 {
            return Err(anyhow::Error::msg(
                "Transaction size must be at least 9 bytes",
            ));
        }

        // Connect to the mempool.
        let stream = TcpStream::connect(self.target)
            .await
            .context(format!("failed to connect to {}", self.target))?;

        // Submit all transactions.
        let burst = self.rate / PRECISION;
        let mut tx = BytesMut::with_capacity(self.size);
        let mut counter = 0;
        let mut r = rand::thread_rng().gen();
        let mut transport = Framed::new(stream, LengthDelimitedCodec::new());
        let interval = interval(Duration::from_millis(BURST_DURATION));
        tokio::pin!(interval);
        let mut evm_generator = self
            .evm
            .as_ref()
            .map(EvmClientConfig::build_generator)
            .transpose()?;
        let mut evm_queue = VecDeque::<TransactionWithHint>::new();

        // NOTE: This log entry is used to compute performance.
        info!("Start sending transactions");

        'main: loop {
            interval.as_mut().tick().await;
            let now = Instant::now();

            for x in 0..burst {
                let bytes = if let Some(generator) = evm_generator.as_mut() {
                    if evm_queue.is_empty() {
                        for (raw_hex, caller) in generator.generate_transactions() {
                            let timestamp = current_timestamp_bytes();
                            let sample_id = u64::from_be_bytes(timestamp);
                            info!("Sending sample transaction {}", sample_id);
                            evm_queue.push_back(TransactionWithHint {
                                timestamp,
                                raw_hex,
                                caller,
                                hint: "autobahn".to_string(),
                            });
                        }
                    }
                    let txn = evm_queue
                        .pop_front()
                        .context("PEVM generator produced no transactions")?;
                    bytes::Bytes::from(
                        bincode::serialize(&txn).context("Failed to serialize PEVM transaction")?,
                    )
                } else {
                    if x == counter % burst {
                        // NOTE: This log entry is used to compute performance.
                        info!("Sending sample transaction {}", counter);

                        tx.put_u8(0u8); // Sample txs start with 0.
                        tx.put_u64(counter); // This counter identifies the tx.
                    } else {
                        r += 1;
                        tx.put_u8(1u8); // Standard txs start with 1.
                        tx.put_u64(r); // Ensures all clients send different txs.
                    };

                    tx.resize(self.size, 0u8);
                    tx.split().freeze()
                };
                if let Err(e) = transport.send(bytes).await {
                    // Uses TCP connection to send requests to one worker.
                    warn!("Failed to send transaction: {}", e);
                    break 'main;
                }
            }
            if now.elapsed().as_millis() > BURST_DURATION as u128 {
                // NOTE: This log entry is used to compute performance.
                warn!("Transaction rate too high for this client");
            }
            counter += 1;
        }
        Ok(())
    }

    pub async fn wait(&self) {
        // Wait for all nodes to be online.
        info!("Waiting for all nodes to be online...");
        join_all(self.nodes.iter().cloned().map(|address| {
            tokio::spawn(async move {
                while TcpStream::connect(address).await.is_err() {
                    sleep(Duration::from_millis(10)).await;
                }
            })
        }))
        .await;
    }
}

fn current_timestamp_bytes() -> [u8; 8] {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    micros.to_be_bytes()
}

struct EvmClientConfig {
    workload: WorkloadType,
    artifacts_dir: PathBuf,
    replica_id: u64,
    replica_num: u64,
}

impl EvmClientConfig {
    fn from_matches(matches: &clap::ArgMatches<'_>) -> Result<Option<Self>> {
        let workload_name = match matches.value_of("workload") {
            Some(name) => name.to_ascii_lowercase(),
            None => return Ok(None),
        };

        let num_clusters = matches
            .value_of("num-clusters")
            .unwrap_or("5")
            .parse()
            .context("The number of clusters must be a positive integer")?;
        let families_per_cluster = matches
            .value_of("families-per-cluster")
            .unwrap_or("5")
            .parse()
            .context("The number of families per cluster must be a positive integer")?;
        let people_per_family = matches
            .value_of("people-per-family")
            .unwrap_or("8")
            .parse()
            .context("The number of people per family must be a positive integer")?;
        let workload = match workload_name.as_str() {
            "erc20" => WorkloadType::ERC20(num_clusters, families_per_cluster, people_per_family),
            "weth" => WorkloadType::WETH(num_clusters, families_per_cluster, people_per_family),
            "uniswap" => {
                WorkloadType::Uniswap(num_clusters, families_per_cluster, people_per_family)
            }
            other => return Err(anyhow::anyhow!("unsupported workload '{other}'")),
        };
        let artifacts_dir = matches
            .value_of("artifacts")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .to_path_buf()
            });
        let replica_id = matches
            .value_of("replica-id")
            .unwrap_or("0")
            .parse()
            .context("The replica id must be a non-negative integer")?;
        let replica_num = matches
            .value_of("replica-num")
            .unwrap_or("1")
            .parse()
            .context("The replica num must be a positive integer")?;

        Ok(Some(Self {
            workload,
            artifacts_dir,
            replica_id,
            replica_num,
        }))
    }

    fn build_generator(&self) -> Result<PevmTransactionGenerator> {
        let (storage_name, addresses_name) = self.workload.artifact_file_names();
        let storage_path = self.artifacts_dir.join(storage_name);
        let addresses_path = self.artifacts_dir.join(addresses_name);
        ensure_workload_artifacts(
            &self.workload,
            &storage_path.to_string_lossy(),
            &addresses_path.to_string_lossy(),
        )?;

        let (tx_sender, _tx_receiver) = mpsc::channel(1);
        let (_signal_sender, signal_receiver) = mpsc::channel(1);
        Ok(PevmTransactionGenerator::new(
            self.workload.clone(),
            self.replica_id,
            self.replica_num,
            tx_sender,
            signal_receiver,
            addresses_path.to_string_lossy().into_owned(),
        ))
    }
}
