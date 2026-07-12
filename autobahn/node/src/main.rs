#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
// Copyright(C) Facebook, Inc. and its affiliates.
use anyhow::{Context, Result};
use clap::{crate_name, crate_version, App, AppSettings, ArgMatches, SubCommand};
use config::Export as _;
use config::Import as _;
use config::{Committee, KeyPair, Parameters, WorkerId};
use crypto::SignatureService;
use env_logger::Env;
use pevm::api::{ensure_workload_artifacts, WorkloadType};
use primary::Header;
use primary::Primary;
use std::path::PathBuf;
use store::Store;
use tokio::sync::mpsc::{channel, Receiver};
use worker::Worker;

/// The default channel capacity.
pub const CHANNEL_CAPACITY: usize = 1_000;

#[tokio::main]
async fn main() -> Result<()> {
    //std::env::set_var("RUST_BACKTRACE", "1");

    let matches = App::new(crate_name!())
        .version(crate_version!())
        .about("A research implementation of Sailfish.")
        .args_from_usage("-v... 'Sets the level of verbosity'")
        .subcommand(
            SubCommand::with_name("generate_keys")
                .about("Print a fresh key pair to file")
                .args_from_usage("--filename=<FILE> 'The file where to print the new key pair'"),
        )
        .subcommand(
            SubCommand::with_name("prepare_workload")
                .about("Materialize a canonical PEVM workload state")
                .args_from_usage("--workload=<NAME> 'PEVM workload: erc20|weth|uniswap'")
                .args_from_usage("--artifacts=<PATH> 'Directory for PEVM workload artifacts'")
                .args_from_usage("--num-clusters=<INT> 'Number of workload clusters'")
                .args_from_usage("--families-per-cluster=<INT> 'Number of families per cluster'")
                .args_from_usage("--people-per-family=<INT> 'Number of people per family'"),
        )
        .subcommand(
            SubCommand::with_name("run")
                .about("Run a node")
                .args_from_usage("--keys=<FILE> 'The file containing the node keys'")
                .args_from_usage("--committee=<FILE> 'The file containing committee information'")
                .args_from_usage("--parameters=[FILE] 'The file containing the node parameters'")
                .args_from_usage("--store=<PATH> 'The path where to create the data store'")
                .args_from_usage(
                    "--execution=[MODE] 'EVM execution mode: none|ordered|speculative'",
                )
                .args_from_usage("--workload=[NAME] 'EVM workload: erc20|weth|uniswap'")
                .args_from_usage("--executor=[MODE] 'PEVM executor mode: sequential|parallel'")
                .args_from_usage("--artifacts=[PATH] 'Directory for PEVM workload artifacts'")
                .args_from_usage("--num-clusters=[INT] 'Number of workload clusters'")
                .args_from_usage("--families-per-cluster=[INT] 'Number of families per cluster'")
                .args_from_usage("--people-per-family=[INT] 'Number of people per family'")
                .subcommand(SubCommand::with_name("primary").about("Run a single primary"))
                .subcommand(
                    SubCommand::with_name("worker")
                        .about("Run a single worker")
                        .args_from_usage("--id=<INT> 'The worker id'"),
                )
                .setting(AppSettings::SubcommandRequiredElseHelp),
        )
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .get_matches();

    let log_level = match matches.occurrences_of("v") {
        0 => "error",
        1 => "warn",
        2 => "info",
        3 => "debug",
        _ => "trace",
    };
    let mut logger = env_logger::Builder::from_env(Env::default().default_filter_or(log_level));
    #[cfg(feature = "benchmark")]
    logger.format_timestamp_millis();
    logger.init();

    match matches.subcommand() {
        ("generate_keys", Some(sub_matches)) => KeyPair::new()
            .export(sub_matches.value_of("filename").unwrap())
            .context("Failed to generate key pair")?,
        ("prepare_workload", Some(sub_matches)) => prepare_workload(sub_matches)?,
        ("run", Some(sub_matches)) => run(sub_matches).await?,
        _ => unreachable!(),
    }
    Ok(())
}

fn prepare_workload(matches: &ArgMatches<'_>) -> Result<()> {
    let num_clusters = matches
        .value_of("num-clusters")
        .unwrap()
        .parse()
        .context("The number of clusters must be a positive integer")?;
    let families_per_cluster = matches
        .value_of("families-per-cluster")
        .unwrap()
        .parse()
        .context("The number of families per cluster must be a positive integer")?;
    let people_per_family = matches
        .value_of("people-per-family")
        .unwrap()
        .parse()
        .context("The number of people per family must be a positive integer")?;
    let workload = match matches.value_of("workload").unwrap() {
        "erc20" => WorkloadType::ERC20(num_clusters, families_per_cluster, people_per_family),
        "weth" => WorkloadType::WETH(num_clusters, families_per_cluster, people_per_family),
        "uniswap" => WorkloadType::Uniswap(num_clusters, families_per_cluster, people_per_family),
        other => return Err(anyhow::anyhow!("unsupported workload '{other}'")),
    };

    let artifacts_dir = PathBuf::from(matches.value_of("artifacts").unwrap());
    std::fs::create_dir_all(&artifacts_dir)
        .context("Failed to create the workload artifact directory")?;
    let (storage_name, addresses_name) = workload.artifact_file_names();
    let storage_path = artifacts_dir.join(storage_name);
    let addresses_path = artifacts_dir.join(addresses_name);
    ensure_workload_artifacts(
        &workload,
        &storage_path.to_string_lossy(),
        &addresses_path.to_string_lossy(),
    )
    .context("Failed to prepare PEVM workload artifacts")?;
    Ok(())
}

// Runs either a worker or a primary.
async fn run(matches: &ArgMatches<'_>) -> Result<()> {
    let key_file = matches.value_of("keys").unwrap();
    let committee_file = matches.value_of("committee").unwrap();
    let parameters_file = matches.value_of("parameters");
    let store_path = matches.value_of("store").unwrap();

    // Read the committee and node's keypair from file.
    let keypair = KeyPair::import(key_file).context("Failed to load the node's keypair")?;
    let name = keypair.name;
    let committee =
        Committee::import(committee_file).context("Failed to load the committee information")?;

    // Load default parameters if none are specified.
    let parameters = match parameters_file {
        Some(filename) => {
            Parameters::import(filename).context("Failed to load the node's parameters")?
        }
        None => Parameters::default(),
    };
    let mut parameters = parameters;

    if let Some(value) = matches.value_of("execution") {
        parameters.evm_execution_mode = value.to_string();
    }
    if let Some(value) = matches.value_of("workload") {
        parameters.evm_workload = value.to_string();
    }
    if let Some(value) = matches.value_of("executor") {
        parameters.evm_executor_mode = value.to_string();
    }
    if let Some(value) = matches.value_of("artifacts") {
        parameters.evm_artifacts_dir = value.to_string();
    }
    if let Some(value) = matches.value_of("num-clusters") {
        parameters.evm_num_clusters = value
            .parse()
            .context("The number of clusters must be a positive integer")?;
    }
    if let Some(value) = matches.value_of("families-per-cluster") {
        parameters.evm_num_families_per_cluster = value
            .parse()
            .context("The number of families per cluster must be a positive integer")?;
    }
    if let Some(value) = matches.value_of("people-per-family") {
        parameters.evm_num_people_per_family = value
            .parse()
            .context("The number of people per family must be a positive integer")?;
    }

    // The `SignatureService` provides signatures on input digests.
    let signature_service = SignatureService::new(keypair.secret);

    // Make the data store.
    let store = Store::new(store_path).context("Failed to create a store")?;

    // Channels the sequence of certificates.
    let (tx_output, rx_output) = channel(CHANNEL_CAPACITY);

    // Channel for sending headers between DAG and Consensus
    let (tx_sailfish, rx_sailfish) = channel(CHANNEL_CAPACITY);

    // Channel for sending loopback headerds that completed validation between DAG and Consensus
    //let (tx_validation, rx_validation) = channel(CHANNEL_CAPACITY);

    // Channel for indicating commit and that new header should be proposed
    //let (tx_ticket, rx_ticket) = channel(CHANNEL_CAPACITY);

    // Check whether to run a primary, a worker, or an entire authority.
    //Note: Each node has at most one worker. Workers that don't include a primary (e.g. are not an entire authority) use PrimaryConnector to connect to a designated primary.
    match matches.subcommand() {
        // Spawn the primary and consensus core.
        ("primary", _) => {
            let (tx_new_certificates, rx_new_certificates) = channel(CHANNEL_CAPACITY);
            let (tx_feedback, rx_feedback) = channel(CHANNEL_CAPACITY);
            let (tx_committer, rx_committer) = channel(CHANNEL_CAPACITY);
            let (tx_pushdown_cert, rx_pushdown_cert) = channel(CHANNEL_CAPACITY);
            let (tx_request_header_sync, rx_request_header_sync) = channel(CHANNEL_CAPACITY);

            Primary::spawn(
                name,
                committee.clone(),
                parameters.clone(),
                signature_service.clone(),
                store.clone(),
                /* tx_consensus */ tx_new_certificates,
                tx_committer,
                rx_committer,
                /* rx_consensus */ rx_feedback,
                tx_sailfish,
                //rx_ticket,
                rx_pushdown_cert,
                rx_request_header_sync,
                tx_output,
            );
            /*Consensus::spawn(
                name,
                committee,
                parameters,
                signature_service,
                store,
                /* rx_consensus */ rx_new_certificates,
                rx_committer,
                /* tx_mempool */ tx_feedback,
                tx_output,
                tx_ticket,
                tx_validation,
                rx_sailfish,
                tx_pushdown_cert,
                tx_request_header_sync,
            );*/
        }

        // Spawn a single worker.
        ("worker", Some(sub_matches)) => {
            let id = sub_matches
                .value_of("id")
                .unwrap()
                .parse::<WorkerId>()
                .context("The worker id must be a positive integer")?;
            Worker::spawn(keypair.name, id, committee, parameters, store);
        }
        _ => unreachable!(),
    }

    // Analyze the consensus' output.
    analyze(rx_output).await;

    // If this expression is reached, the program ends and all other tasks terminate.
    unreachable!();
}

/// Receives an ordered list of certificates and apply any application-specific logic.
async fn analyze(mut rx_output: Receiver<Header>) {
    while let Some(_header) = rx_output.recv().await {
        // NOTE: Here goes the application logic.
    }
}
