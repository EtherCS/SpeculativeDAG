# Orchestrator

The Orchestrator crate provides facilities for quickly deploying and benchmarking this codebase in a geo-distributed environment. Please note that it is not intended for production deployments or as an indicator of production engineering best practices. Its purpose is to facilitate research projects by allowing benchmarking of (variants of) the codebase and analyzing performance.

This guide provides a step-by-step explanation of how to run geo-distributed benchmarks on either [Vultr](http://vultr.com) or [Amazon Web Services (AWS)](http://aws.amazon.com).

## Step 1. Set up cloud provider credentials

To enable programmatic access to your cloud provider account from your local machine, you need to set up your cloud provider credentials. These credentials authorize your machine to create, delete, and edit instances programmatically on your account.

### Setting up Vultr credentials

1. Find your ['Vultr token'](https://www.vultr.com/docs/).
2. Create a file `~/.vultr` and add your access token as the file's content:

```text
YOUR_ACCESS_TOKEN
```

### Setting up AWS credentials

1. Find your ['access key id' and 'secret access key'](https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-quickstart.html#cli-configure-quickstart-creds).
2. Create a file `~/.aws/credentials` with the following content:

```text
[default]
aws_access_key_id = YOUR_ACCESS_KEY_ID
aws_secret_access_key = YOUR_SECRET_ACCESS_KEY
```

Do not specify any AWS region in that file, as the scripts need to handle multiple regions programmatically.

## Step 2. Specify the testbed configuration

Create a file called `settings.yml` that contains all the configuration parameters for the testbed deployment. You can find an example file at `./assets/settings-template.yml`.

The documentation of the `Settings` struct in `./src/settings.rs` provides detailed information about each field and indicates which ones are optional. If you're working with a private GitHub repository, you can include a [private access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) in the repository URL. For example, if your access token is `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`, the repository URL should be formatted as follows:

```yml
repository:
  - url: http://YOUR_ACCESS_TOKEN@github.com/asonino/-mysticeti.git
  - commit: main
```

## Step 3. Create a testbed

The `orchestrator` binary provides various functionalities for creating, starting, stopping, and destroying instances. You can use the following command to boot 2 instances per region (if the settings file specifies 10 regions, as shown in the example above, a total of 20 instances will be created):

```bash
cargo run --bin orchestrator -- testbed deploy --instances 2
```

To check the current status of the testbed instances, use the following command:

```bash
cargo run --bin orchestrator testbed status
```

Instances listed with a green number are available and ready for use, while instances listed with a red number are stopped.

## Step 4. Running benchmarks

Running benchmarks involves installing the specified version of the codebase on the remote machines and running one validator and one load generator per instance. For example, the following command benchmarks a committee of 10 validators under a constant load of 200 tx/s:

```bash
cargo run --bin orchestrator -- benchmark --committee 10 --loads 200
```

In a network of 10 validators, each with a corresponding load generator, each load generator submits a fixed load of 20 tx/s. Performance measurements are collected by regularly scraping the Prometheus metrics exposed by the load generators. The `orchestrator` binary provides additional commands to run a specific number of load generators on separate machines.

### Consecutive commit-stall attack

Use `--stall-start` and `--stall-end` to suppress direct leader decisions during a deterministic
interval while normal DAG construction continues:

```bash
cargo run --release --bin orchestrator -- \
  benchmark --committee 10 --loads 400 \
  --mode full --workload erc20 --stall-start 10 --stall-end 45
```

Both values are seconds after validator startup. They must be provided together, `stall-end` must
be greater than `stall-start`, and the `benchmark_duration` in `settings.yml` must be greater than
`stall-end` to leave time to observe recovery. The interval can alternatively be specified as
`direct_commit_stall_simulation` in the node-parameters YAML; CLI values override the YAML value.

Use `--mode` to select `full` (default), `eac`, `no-snapshots`, or `eager-snapshots`. Use
`--workload` to select `erc20` (default), `weth`, or `uniswap`. The selected workload controls both
the PEVM workload type and the storage/address artifacts uploaded during genesis. Mode and workload
are included in measurement and log directory names.

## Step 5. Monitoring

The orchestrator provides facilities to monitor metrics on clients and nodes. It deploys a [Prometheus](https://prometheus.io) instance and a [Grafana](https://grafana.com) instance on a dedicated remote machine. Grafana is then available on the address printed on stdout (e.g., `http://3.83.97.12:3000`) with the default username and password both set to `admin`. You can either create a [new dashboard](https://grafana.com/docs/grafana/latest/getting-started/build-first-dashboard/) or [import](https://grafana.com/docs/grafana/latest/dashboards/manage-dashboards/#import-a-dashboard) the example dashboard located in the `./assets` folder.

The setup also installs a lightweight validator resource collector on every node. It exports
`mysticeti_process_cpu_percent` and `mysticeti_process_resident_memory_bytes` through node-exporter.
The default dashboard displays per-node validator CPU and RSS immediately below the latency row.
CPU uses `ps` semantics and may exceed 100% for a multi-core process; RSS excludes unrelated host
processes.
