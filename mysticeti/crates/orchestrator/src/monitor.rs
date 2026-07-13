// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{fs, net::SocketAddr, path::PathBuf};

use crate::{
    benchmark::BenchmarkParameters,
    client::Instance,
    error::{MonitorError, MonitorResult},
    protocol::ProtocolMetrics,
    ssh::{CommandContext, SshConnectionManager},
};

pub struct Monitor {
    instance: Instance,
    clients: Vec<Instance>,
    nodes: Vec<Instance>,
    ssh_manager: SshConnectionManager,
}

impl Monitor {
    /// Create a new monitor.
    pub fn new(
        instance: Instance,
        clients: Vec<Instance>,
        nodes: Vec<Instance>,
        ssh_manager: SshConnectionManager,
    ) -> Self {
        Self {
            instance,
            clients,
            nodes,
            ssh_manager,
        }
    }

    /// Dependencies to install.
    pub fn dependencies() -> Vec<String> {
        let mut commands: Vec<String> = Vec::new();
        commands.extend(Prometheus::install_commands().into_iter().map(String::from));
        commands.extend(Grafana::install_commands().into_iter().map(String::from));
        commands.extend(ResourceExporter::install_commands());
        commands.extend(NodeExporter::install_commands());
        commands
    }

    /// Start a prometheus instance on the dedicated motoring machine.
    pub async fn start_prometheus<P: ProtocolMetrics>(
        &self,
        protocol_commands: &P,
        parameters: &BenchmarkParameters,
    ) -> MonitorResult<()> {
        // Configure and reload prometheus.
        let instance = [self.instance.clone()];
        let commands = Prometheus::setup_commands(
            self.nodes.clone(),
            self.clients.clone(),
            protocol_commands,
            parameters,
        );
        self.ssh_manager
            .execute(instance, commands, CommandContext::default())
            .await?;

        Ok(())
    }

    /// Start grafana on the dedicated motoring machine.
    pub async fn start_grafana(&self) -> MonitorResult<()> {
        // Configure and reload grafana.
        let instance = std::iter::once(self.instance.clone());
        let commands = Grafana::setup_commands();
        self.ssh_manager
            .execute(instance, commands, CommandContext::default())
            .await?;

        Ok(())
    }

    /// The public address of the grafana instance.
    pub fn grafana_address(&self) -> String {
        format!("http://{}:{}", self.instance.main_ip, Grafana::DEFAULT_PORT)
    }
}

/// Generate the commands to setup prometheus on the given instances.
pub struct Prometheus;

impl Prometheus {
    /// The default prometheus configuration path.
    const DEFAULT_PROMETHEUS_CONFIG_PATH: &'static str = "/etc/prometheus/prometheus.yml";
    /// The default prometheus port.
    pub const DEFAULT_PORT: u16 = 9090;

    /// The commands to install prometheus.
    pub fn install_commands() -> Vec<&'static str> {
        vec![
            "sudo apt-get -y install prometheus",
            "sudo chmod 777 -R /var/lib/prometheus/ /etc/prometheus/",
        ]
    }

    /// Generate the commands to update the prometheus configuration and restart prometheus.
    pub fn setup_commands<I, P>(
        nodes: I,
        _clients: I,
        protocol: &P,
        parameters: &BenchmarkParameters,
    ) -> String
    where
        I: IntoIterator<Item = Instance>,
        P: ProtocolMetrics,
    {
        // Generate the prometheus configuration.
        let mut config = vec![Self::global_configuration()];

        let nodes_metrics_path = protocol.nodes_metrics_path(nodes, parameters);
        for (i, (_, nodes_metrics_path)) in nodes_metrics_path.into_iter().enumerate() {
            let id = format!("node-{i}");
            let scrape_config = Self::scrape_configuration(&id, &nodes_metrics_path);
            config.push(scrape_config);
        }

        // NOTE: Hack to avoid clients metrics.
        // let clients_metrics_path = protocol.clients_metrics_path(clients, parameters);
        // for (i, (_, client_metrics_path)) in clients_metrics_path.into_iter().enumerate() {
        //     let id = format!("client-{i}");
        //     let scrape_config = Self::scrape_configuration(&id, &client_metrics_path);
        //     config.push(scrape_config);
        // }

        // Make the command to configure and restart prometheus.
        format!(
            "sudo echo \"{}\" > {} && sudo service prometheus restart",
            config.join("\n"),
            Self::DEFAULT_PROMETHEUS_CONFIG_PATH
        )
    }

    /// Generate the global prometheus configuration.
    /// NOTE: The configuration file is a yaml file so spaces are important.
    fn global_configuration() -> String {
        [
            "global:",
            "  scrape_interval: 5s",
            "  evaluation_interval: 5s",
            "scrape_configs:",
        ]
        .join("\n")
    }

    /// Generate the prometheus configuration from the given metrics path.
    /// NOTE: The configuration file is a yaml file so spaces are important.
    fn scrape_configuration(id: &str, nodes_metrics_path: &str) -> String {
        let parts: Vec<_> = nodes_metrics_path.split('/').collect();
        let address = parts[0].parse::<SocketAddr>().unwrap();
        let ip = address.ip();
        let port = address.port();
        let path = parts[1];

        [
            &format!("  - job_name: instance-{id}"),
            &format!("    metrics_path: /{path}"),
            "    static_configs:",
            "      - targets:",
            &format!("        - {ip}:{port}"),
            &format!("  - job_name: instance-node-exporter-{id}"),
            "    static_configs:",
            "      - targets:",
            &format!("        - {ip}:9200"),
        ]
        .join("\n")
    }
}

pub struct Grafana;

impl Grafana {
    /// The path to the datasources directory.
    const DATASOURCES_PATH: &'static str = "/etc/grafana/provisioning/datasources";
    /// The default grafana port.
    pub const DEFAULT_PORT: u16 = 3000;

    /// The commands to install grafana.
    pub fn install_commands() -> Vec<&'static str> {
        vec![
            "sudo apt-get install -y apt-transport-https software-properties-common wget",
            "sudo wget -q -O /etc/apt/keyrings/grafana.key https://apt.grafana.com/gpg.key",
            "(sudo rm /etc/apt/sources.list.d/grafana.list || true)",
            "echo \
                \"deb [signed-by=/etc/apt/keyrings/grafana.key] \
                https://apt.grafana.com stable main\" \
                | sudo tee -a /etc/apt/sources.list.d/grafana.list",
            "sudo apt-get update",
            "sudo apt-get install -y grafana",
            "sudo chmod 777 -R /etc/grafana/",
        ]
    }

    /// Generate the commands to update the grafana datasource and restart grafana.
    pub fn setup_commands() -> String {
        [
            &format!("(rm -r {} || true)", Self::DATASOURCES_PATH),
            &format!("mkdir -p {}", Self::DATASOURCES_PATH),
            &format!(
                "sudo echo \"{}\" > {}/testbed.yml",
                Self::datasource(),
                Self::DATASOURCES_PATH
            ),
            "sudo service grafana-server restart",
        ]
        .join(" && ")
    }

    /// Generate the content of the datasource file for the given instance.
    /// NOTE: The datasource file is a yaml file so spaces are important.
    fn datasource() -> String {
        [
            "apiVersion: 1",
            "deleteDatasources:",
            "  - name: testbed",
            "    orgId: 1",
            "datasources:",
            "  - name: testbed",
            "    type: prometheus",
            "    access: proxy",
            "    orgId: 1",
            &format!("    url: http://localhost:{}", Prometheus::DEFAULT_PORT),
            "    editable: true",
            "    uid: Fixed-UID-testbed",
        ]
        .join("\n")
    }
}

#[allow(dead_code)] // TODO: Will be used to observe local testbeds (#8)
/// Bootstrap the grafana with datasource to connect to the given instances.
/// NOTE: Only for macOS. Grafana must be installed through homebrew (and not from source).
/// Deeper grafana configuration can be done through the grafana.ini file
/// (/opt/homebrew/etc/grafana/grafana.ini) or the plist file
/// (~/Library/LaunchAgents/homebrew.mxcl.grafana.plist).
pub struct LocalGrafana;

#[allow(dead_code)] // TODO: Will be used to observe local testbeds (#8)
impl LocalGrafana {
    /// The default grafana home directory (macOS, homebrew install).
    const DEFAULT_GRAFANA_HOME: &'static str = "/opt/homebrew/opt/grafana/share/grafana/";
    /// The path to the datasources directory.
    const DATASOURCES_PATH: &'static str = "conf/provisioning/datasources/";
    /// The default grafana port.
    pub const DEFAULT_PORT: u16 = 3000;

    /// Configure grafana to connect to the given instances. Only for macOS.
    pub fn run<I>(instances: I) -> MonitorResult<()>
    where
        I: IntoIterator<Item = Instance>,
    {
        let path: PathBuf = [Self::DEFAULT_GRAFANA_HOME, Self::DATASOURCES_PATH]
            .iter()
            .collect();

        // Remove the old datasources.
        fs::remove_dir_all(&path).unwrap();
        fs::create_dir(&path).unwrap();

        // Create the new datasources.
        for (i, instance) in instances.into_iter().enumerate() {
            let mut file = path.clone();
            file.push(format!("instance-{}.yml", i));
            fs::write(&file, Self::datasource(&instance, i)).map_err(|e| {
                MonitorError::GrafanaError(format!("Failed to write grafana datasource ({e})"))
            })?;
        }

        // Restart grafana.
        std::process::Command::new("brew")
            .arg("services")
            .arg("restart")
            .arg("grafana")
            .arg("-q")
            .spawn()
            .map_err(|e| MonitorError::GrafanaError(e.to_string()))?;

        Ok(())
    }

    /// Generate the content of the datasource file for the given instance. This grafana instance
    /// takes one datasource per instance and assumes one prometheus server runs per instance.
    /// NOTE: The datasource file is a yaml file so spaces are important.
    fn datasource(instance: &Instance, index: usize) -> String {
        [
            "apiVersion: 1",
            "deleteDatasources:",
            &format!("  - name: instance-{index}"),
            "    orgId: 1",
            "datasources:",
            &format!("  - name: instance-{index}"),
            "    type: prometheus",
            "    access: proxy",
            "    orgId: 1",
            &format!(
                "    url: http://{}:{}",
                instance.main_ip,
                Prometheus::DEFAULT_PORT
            ),
            "    editable: true",
            &format!("    uid: UID-{index}"),
        ]
        .join("\n")
    }
}

/// Generate the commands to setup node exporter on the given instances.
struct NodeExporter;

impl NodeExporter {
    const RELEASE: &'static str = "1.8.2";
    const DEFAULT_PORT: u16 = 9200;
    const SERVICE_PATH: &'static str = "/etc/systemd/system/node_exporter.service";
    const TEXTFILE_DIR: &'static str = "/var/lib/node_exporter/textfile_collector";

    pub fn install_commands() -> Vec<String> {
        let install = format!(
            "if ! command -v node_exporter >/dev/null; then \
             case $(uname -m) in x86_64) arch=amd64 ;; aarch64|arm64) arch=arm64 ;; *) echo unsupported architecture >&2; exit 1 ;; esac; \
             build=node_exporter-{release}.linux-$arch; \
             curl -fLO https://github.com/prometheus/node_exporter/releases/download/v{release}/$build.tar.gz; \
             tar -xzf $build.tar.gz; \
             sudo mv $build/node_exporter /usr/local/bin/; \
             fi",
            release = Self::RELEASE,
        );

        [
            &install,
            "sudo useradd -rs /bin/false node_exporter || true",
            &format!("sudo mkdir -p {}", Self::TEXTFILE_DIR),
            &format!("sudo chmod 755 {}", Self::TEXTFILE_DIR),
            "sudo chmod 777 -R /etc/systemd/system/",
            &format!(
                "sudo echo \"{}\" > {}",
                Self::service_config(),
                Self::SERVICE_PATH
            ),
            "sudo systemctl daemon-reload",
            "sudo systemctl restart node_exporter",
            "sudo systemctl enable node_exporter",
        ]
        .map(|x| x.to_string())
        .to_vec()
    }

    fn service_config() -> String {
        [
            "[Unit]",
            "Description=Node Exporter",
            "After=network.target",
            "[Service]",
            "User=node_exporter",
            "Group=node_exporter",
            "Type=simple",
            &format!(
                "ExecStart=/usr/local/bin/node_exporter --web.listen-address=:{} --collector.textfile.directory={}",
                Self::DEFAULT_PORT,
                Self::TEXTFILE_DIR,
            ),
            "[Install]",
            "WantedBy=multi-user.target",
        ]
        .join("\n")
    }
}

/// Export resource usage of the validator process through node-exporter's textfile collector.
struct ResourceExporter;

impl ResourceExporter {
    const SCRIPT_PATH: &'static str = "/usr/local/bin/mysticeti_resource_exporter";
    const SERVICE_PATH: &'static str = "/etc/systemd/system/mysticeti_resource_exporter.service";

    pub fn install_commands() -> Vec<String> {
        vec![
            format!(
                "printf %s {} | sudo tee {} >/dev/null",
                Self::shell_quote(&Self::script()),
                Self::SCRIPT_PATH,
            ),
            format!("sudo chmod 755 {}", Self::SCRIPT_PATH),
            format!(
                "printf %s {} | sudo tee {} >/dev/null",
                Self::shell_quote(&Self::service_config()),
                Self::SERVICE_PATH,
            ),
            "sudo systemctl daemon-reload".to_string(),
            "sudo systemctl restart mysticeti_resource_exporter".to_string(),
            "sudo systemctl enable mysticeti_resource_exporter".to_string(),
        ]
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }

    fn script() -> String {
        format!(
            r##"#!/usr/bin/env bash
set -u

output_dir="{}"
output_file="${{output_dir}}/mysticeti_process.prom"
sudo mkdir -p "${{output_dir}}"

while true; do
    pid=$(pgrep -n -f "[m]ysticeti run" || true)
    cpu_percent=0
    rss_kb=0
    if [[ -n "${{pid}}" ]]; then
        read -r cpu_percent rss_kb < <(ps -p "${{pid}}" -o %cpu= -o rss= 2>/dev/null || echo "0 0")
    fi
    rss_bytes=$(( ${{rss_kb:-0}} * 1024 ))
    temporary_file="${{output_file}}.$$"
    {{
        echo "# HELP mysticeti_process_cpu_percent CPU utilization of the Mysticeti validator process"
        echo "# TYPE mysticeti_process_cpu_percent gauge"
        echo "mysticeti_process_cpu_percent ${{cpu_percent:-0}}"
        echo "# HELP mysticeti_process_resident_memory_bytes Resident set size of the Mysticeti validator process"
        echo "# TYPE mysticeti_process_resident_memory_bytes gauge"
        echo "mysticeti_process_resident_memory_bytes ${{rss_bytes}}"
    }} > "${{temporary_file}}"
    mv "${{temporary_file}}" "${{output_file}}"
    sleep 1
done
"##,
            NodeExporter::TEXTFILE_DIR
        )
    }

    fn service_config() -> String {
        [
            "[Unit]",
            "Description=Mysticeti process resource exporter",
            "After=network.target",
            "[Service]",
            "Type=simple",
            &format!("ExecStart={}", Self::SCRIPT_PATH),
            "Restart=always",
            "RestartSec=1",
            "[Install]",
            "WantedBy=multi-user.target",
        ]
        .join("\n")
    }
}

#[cfg(test)]
mod resource_exporter_tests {
    use super::*;

    #[test]
    fn exports_validator_cpu_and_rss_metrics() {
        let script = ResourceExporter::script();
        assert!(script.contains("pgrep -n -f \"[m]ysticeti run\""));
        assert!(script.contains("mysticeti_process_cpu_percent"));
        assert!(script.contains("mysticeti_process_resident_memory_bytes"));
    }

    #[test]
    fn node_exporter_reads_textfile_metrics() {
        let service = NodeExporter::service_config();
        assert!(service.contains("--collector.textfile.directory="));
        assert!(service.contains(NodeExporter::TEXTFILE_DIR));
    }
}
