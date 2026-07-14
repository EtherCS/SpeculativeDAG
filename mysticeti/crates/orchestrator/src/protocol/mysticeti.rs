// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    fmt::{Debug, Display},
    net::IpAddr,
    ops::Deref,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::ValueEnum;
use mysticeti_core::{
    config::{
        self, ClientParameters, DirectCommitStallSimulation, NodeParameters,
        SpeculationPredictionPolicy, SpeculationSnapshotPolicy,
    },
    types::AuthorityIndex,
};
use serde::{Deserialize, Serialize};

use super::{ProtocolCommands, ProtocolMetrics, ProtocolParameters, BINARY_PATH};
use crate::{benchmark::BenchmarkParameters, client::Instance, settings::Settings};

const COORDINATED_START_GRACE_PERIOD: Duration = Duration::from_secs(60);

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct MysticetiNodeParameters(NodeParameters);

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum MysticetiMode {
    Full,
    Eac,
    NoSnapshots,
    EagerSnapshots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum MysticetiWorkload {
    Erc20,
    Weth,
    Uniswap,
}

impl Deref for MysticetiNodeParameters {
    type Target = NodeParameters;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Debug for MysticetiNodeParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = self.mode_label();
        let workload = self.workload_label();
        if let Some(stall) = &self.direct_commit_stall_simulation {
            write!(
                f,
                "{mode}-{workload}-attack-{}-{}",
                stall.start_time.as_secs(),
                stall.start_time.saturating_add(stall.duration).as_secs()
            )
        } else {
            write!(f, "{mode}-{workload}")
        }
    }
}

impl Display for MysticetiNodeParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.consensus_only {
            write!(f, "Consensus-only mode")
        } else {
            write!(f, "FPC mode")
        }
    }
}

impl ProtocolParameters for MysticetiNodeParameters {}

impl MysticetiNodeParameters {
    pub fn apply_benchmark_profile(&mut self, mode: MysticetiMode, workload: MysticetiWorkload) {
        self.0.pevm_workload_type = match workload {
            MysticetiWorkload::Erc20 => pevm::api::WorkloadType::ERC20(5, 5, 8),
            MysticetiWorkload::Weth => pevm::api::WorkloadType::WETH(5, 5, 8),
            MysticetiWorkload::Uniswap => pevm::api::WorkloadType::Uniswap(5, 5, 8),
        };

        match mode {
            MysticetiMode::Full => {
                self.0.enable_speculative_execution = true;
                self.0.speculation_prediction_policy = SpeculationPredictionPolicy::Adaptive;
                self.0.speculation_snapshot_policy = SpeculationSnapshotPolicy::Adaptive;
            }
            MysticetiMode::Eac => {
                self.0.enable_speculative_execution = false;
                self.0.speculation_prediction_policy = SpeculationPredictionPolicy::Adaptive;
                self.0.speculation_snapshot_policy = SpeculationSnapshotPolicy::None;
            }
            MysticetiMode::NoSnapshots => {
                self.0.enable_speculative_execution = true;
                self.0.speculation_prediction_policy = SpeculationPredictionPolicy::Adaptive;
                self.0.speculation_snapshot_policy = SpeculationSnapshotPolicy::None;
            }
            MysticetiMode::EagerSnapshots => {
                self.0.enable_speculative_execution = true;
                self.0.speculation_prediction_policy = SpeculationPredictionPolicy::Adaptive;
                self.0.speculation_snapshot_policy = SpeculationSnapshotPolicy::Eager;
            }
        }
    }

    pub fn set_direct_commit_stall(&mut self, start: Duration, end: Duration) {
        self.0.direct_commit_stall_simulation = Some(DirectCommitStallSimulation::new(
            start,
            end.saturating_sub(start),
        ));
    }

    fn mode_label(&self) -> &'static str {
        if !self.enable_speculative_execution {
            "eac"
        } else {
            match self.speculation_snapshot_policy {
                SpeculationSnapshotPolicy::Adaptive => "full",
                SpeculationSnapshotPolicy::None => "no-snapshots",
                SpeculationSnapshotPolicy::Eager => "eager-snapshots",
            }
        }
    }

    fn workload_label(&self) -> &'static str {
        match self.pevm_workload_type {
            pevm::api::WorkloadType::ERC20(_, _, _) => "erc20",
            pevm::api::WorkloadType::WETH(_, _, _) => "weth",
            pevm::api::WorkloadType::Uniswap(_, _, _) => "uniswap",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(transparent)]
pub struct MysticetiClientParameters(ClientParameters);

impl Deref for MysticetiClientParameters {
    type Target = ClientParameters;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Debug for MysticetiClientParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.transaction_size)
    }
}

impl Display for MysticetiClientParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}B tx", self.transaction_size)
    }
}

impl ProtocolParameters for MysticetiClientParameters {}

pub struct MysticetiProtocol {
    working_dir: PathBuf,
    account_storage_path: String,
    account_addresses_path: String,
}

impl ProtocolCommands for MysticetiProtocol {
    fn protocol_dependencies(&self) -> Vec<&'static str> {
        vec!["sudo apt -y install libfontconfig1-dev"]
    }

    fn db_directories(&self) -> Vec<std::path::PathBuf> {
        vec![self.working_dir.join("storage-*")]
    }

    async fn genesis_command<'a, I>(&self, instances: I, parameters: &BenchmarkParameters) -> String
    where
        I: Iterator<Item = &'a Instance>,
    {
        let ips = instances
            .map(|x| x.main_ip.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        let node_parameters = parameters.node_parameters.clone();
        let node_parameters_string = serde_yaml::to_string(&node_parameters).unwrap();
        let node_parameters_path = self.working_dir.join("node-parameters.yaml");
        let upload_node_parameters = format!(
            "echo -e '{node_parameters_string}' > {}",
            node_parameters_path.display()
        );

        let mut client_parameters = parameters.client_parameters.clone();
        client_parameters.0.load = parameters.load / parameters.nodes;
        let client_parameters_string = serde_yaml::to_string(&client_parameters).unwrap();
        let client_parameters_path = self.working_dir.join("client-parameters.yaml");
        let upload_client_parameters = format!(
            "echo -e '{client_parameters_string}' > {}",
            client_parameters_path.display()
        );

        let genesis = [
            &format!("./{BINARY_PATH}/mysticeti"),
            "benchmark-genesis",
            &format!(
                "--ips {ips} --working-directory {} --node-parameters-path {} --account-storage-path {} --account-addresses-path {}",
                self.working_dir.display(),
                node_parameters_path.display(),
                self.account_storage_path,
                self.account_addresses_path,
            ),
        ]
        .join(" ");

        [
            "source $HOME/.cargo/env",
            &upload_node_parameters,
            &upload_client_parameters,
            &genesis,
        ]
        .join(" && ")
    }

    fn node_command<I>(
        &self,
        instances: I,
        _parameters: &BenchmarkParameters,
    ) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        // SSH process launches can differ by tens of seconds on a large testbed.
        // A shared future start keeps consensus, workload, and attack timers aligned.
        let benchmark_start_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System clock is before the Unix epoch")
            .saturating_add(COORDINATED_START_GRACE_PERIOD)
            .as_millis();

        instances
            .into_iter()
            .enumerate()
            .map(|(i, instance)| {
                let authority = i as AuthorityIndex;
                let committee_path = self.working_dir.join("committee.yaml");
                let public_config_path = self.working_dir.join("public-config.yaml");
                let private_config_path = self
                    .working_dir
                    .join(format!("private-config-{authority}.yaml"));
                let client_parameters_path = self.working_dir.join("client-parameters.yaml");

                let run = [
                    "RUST_LOG=debug",
                    &format!("./{BINARY_PATH}/mysticeti"),
                    "run",
                    &format!("--authority {authority}"),
                    &format!("--committee-path {}", committee_path.display()),
                    &format!("--public-config-path {}", public_config_path.display()),
                    &format!("--private-config-path {}", private_config_path.display()),
                    &format!(
                        "--client-parameters-path {}",
                        client_parameters_path.display()
                    ),
                    &format!("--benchmark-start-unix-ms {benchmark_start_unix_ms}"),
                ]
                .join(" ");

                let command = ["cd mysticeti", "source $HOME/.cargo/env", &run].join(" && ");
                (instance, command)
            })
            .collect()
    }

    fn client_command<I>(
        &self,
        _instances: I,
        _parameters: &BenchmarkParameters,
    ) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        // TODO: Isolate clients from the node (#9).
        vec![]
    }
}

impl ProtocolMetrics for MysticetiProtocol {
    const BENCHMARK_DURATION: &'static str = mysticeti_core::metrics::BENCHMARK_DURATION;
    const TOTAL_TRANSACTIONS: &'static str = "latency_s_count";
    const LATENCY_BUCKETS: &'static str = "latency_s";
    const LATENCY_SUM: &'static str = "latency_s_sum";
    const LATENCY_SQUARED_SUM: &'static str = mysticeti_core::metrics::LATENCY_SQUARED_S;

    fn nodes_metrics_path<I>(
        &self,
        instances: I,
        parameters: &BenchmarkParameters,
    ) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        let (ips, instances): (_, Vec<_>) = instances
            .into_iter()
            .map(|x| (IpAddr::V4(x.main_ip), x))
            .unzip();

        let node_parameters = Some(parameters.node_parameters.deref().clone());
        let node_config = config::NodePublicConfig::new_for_benchmarks(ips, node_parameters);
        let metrics_paths = node_config
            .all_metric_addresses()
            .map(|x| format!("{x}{}", mysticeti_core::prometheus::METRICS_ROUTE));

        instances.into_iter().zip(metrics_paths).collect()
    }

    fn clients_metrics_path<I>(
        &self,
        instances: I,
        parameters: &BenchmarkParameters,
    ) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        // NOTE: Hack to avoid clients metrics.
        self.nodes_metrics_path(instances, parameters)
    }
}

impl MysticetiProtocol {
    /// Make a new instance of the Mysticeti protocol commands generator.
    pub fn new(settings: &Settings) -> Self {
        Self {
            working_dir: settings.working_dir.clone(),
            account_storage_path: settings
                .account_storage_path
                .clone()
                .unwrap_or_else(|| "storage_5_5_8.json".to_string()),
            account_addresses_path: settings
                .account_addresses_path
                .clone()
                .unwrap_or_else(|| "account_addresses_5_5_8.bin".to_string()),
        }
    }

    pub fn with_workload(mut self, workload: &pevm::api::WorkloadType) -> Self {
        let (storage, addresses) = workload.artifact_file_names();
        self.account_storage_path = storage;
        self.account_addresses_path = addresses;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_commit_stall_is_serialized_and_labeled() {
        let mut parameters = MysticetiNodeParameters::default();
        parameters.apply_benchmark_profile(MysticetiMode::Full, MysticetiWorkload::Erc20);
        parameters.set_direct_commit_stall(Duration::from_secs(10), Duration::from_secs(45));

        let stall = parameters.direct_commit_stall_simulation.as_ref().unwrap();
        assert_eq!(stall.start_time, Duration::from_secs(10));
        assert_eq!(stall.duration, Duration::from_secs(35));
        assert_eq!(format!("{parameters:?}"), "full-erc20-attack-10-45");

        let yaml = serde_yaml::to_string(&parameters).unwrap();
        assert!(yaml.contains("direct_commit_stall_simulation:"));
        assert!(yaml.contains("secs: 35"));
    }

    #[test]
    fn benchmark_profiles_map_to_node_parameters() {
        let cases = [
            (MysticetiMode::Full, "full"),
            (MysticetiMode::Eac, "eac"),
            (MysticetiMode::NoSnapshots, "no-snapshots"),
            (MysticetiMode::EagerSnapshots, "eager-snapshots"),
        ];

        for (mode, label) in cases {
            let mut parameters = MysticetiNodeParameters::default();
            parameters.apply_benchmark_profile(mode, MysticetiWorkload::Uniswap);
            assert_eq!(format!("{parameters:?}"), format!("{label}-uniswap"));
        }
    }
}
