# Pufferfish

[![build status](https://img.shields.io/github/actions/workflow/status/asonnino/shamir-bip39/code.yml?branch=main&logo=github&style=flat-square)](https://github.com/asonnino/shamir-bip39/actions)
[![rustc](https://img.shields.io/badge/rustc-1.78+-blue?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-Apache-blue.svg?style=flat-square)](LICENSE)

The code in this branch is a prototype of Pufferfish.

## Build

```bash
cd mysticeti
cargo build
```

For quick validation after local changes:

```bash
cargo check
```

## Local Experiments

All local experiment wrappers live under [`scripts/`](scripts). They start a local committee with `tmux`, scrape Prometheus metrics from each validator, and write the results into a run directory.

To avoid rebuilding the binary before every run, build once with `cargo build` and then set `SKIP_BUILD=1` in the wrappers below.

### Fast Test

Run a short local committee without network jitter:

```bash
SKIP_BUILD=1 bash speculative.sh 4 60 100 full erc20
```

This runs 4 validators for 60 seconds with input 100 tx/s on the `erc20` workload and writes results to `./results/speculative-<mode>-<workload>-<timestamp>/`.

### Jitter Run

Run the system with injected network jitter:

```bash
SKIP_BUILD=1 bash jitterrun.sh 90 7 1 4 2500 10 50 100 full erc20
```

Arguments:

1. total run duration
2. committee size
3. number of jittered validators
4. delayed outgoing connections per jittered validator
5. delay in milliseconds
6. jitter start time in seconds
7. jitter duration in seconds
8. client load
9. experiment mode
10. workload (`erc20`, `weth`, or `uniswap`)

Jitter runs also monitor each validator's CPU and resident memory. The sampling interval defaults to
one second and can be changed with `RESOURCE_MONITOR_INTERVAL`, for example:

```bash
RESOURCE_MONITOR_INTERVAL=2 SKIP_BUILD=1 \
bash jitterrun.sh 90 7 1 4 2500 10 50 100 full erc20
```

Resource samples and average/peak summaries are written to `resource-usage.csv` and
`resource-summary.csv` in the run directory.

### Consecutive Commit-Stall Attack

Run a deterministic interval in which every direct leader decision is held as undecided while
normal DAG construction continues:

```bash
SKIP_BUILD=1 bash scripts/attackrun.sh 90 7 10 60 100 full erc20
```

Arguments:

1. total run duration
2. committee size
3. stall start time in seconds
4. stall end time in seconds
5. client load
6. experiment mode
7. workload (`erc20`, `weth`, or `uniswap`)
8. optional output directory

The run duration must be greater than the stall end time so the experiment captures catch-up after
the attack. Results are written to
`./results/attack-<mode>-<workload>-<committee-size>-<timestamp>/` by default.

This mode emulates certificate withholding at the consensus decision boundary. Blocks continue to
be disseminated normally, so each leader can retain the next-round quorum references used by APS
and replicas continue creating blocks across consecutive rounds. During `[stall_start, stall_end)`,
the direct commit rule returns `Undecided`; after `stall_end`, normal direct and indirect decisions
resume over the accumulated DAG. It is a deterministic fault-injection experiment, not a
packet-level Byzantine network scheduler.

#### AWS Commit-Stall Evaluation

The orchestrator can run the same deterministic attack on AWS. Configure the AWS credentials,
repository branch, node parameters, workload artifacts, and `benchmark_duration` in
`mysticeti/crates/orchestrator/assets/settings.yml`. The benchmark duration must exceed the stall
end so that the run includes a recovery interval.

The attack is disabled by default (`direct_commit_stall_simulation: null`). A normal benchmark
therefore requires no attack-related arguments:

```bash
cargo run --release --bin orchestrator -- benchmark --committee 10 --loads 400
```

From the `mysticeti` directory, run:

```bash
cargo run --release --bin orchestrator -- \
  --settings-path crates/orchestrator/assets/settings.yml \
  benchmark --committee 10 --loads 400 \
  --mode full --workload erc20 \
  --stall-start 10 --stall-end 45
```

Both stall arguments are measured in seconds from each validator's startup and must be specified
together. The orchestrator injects the interval into the public node configuration generated on
AWS. Result and log names include `attack-<start>-<end>` to distinguish attack runs from normal
benchmarks.

AWS benchmarks accept the same execution modes and workloads used by local evaluations:

- `--mode full` (default): adaptive prediction and adaptive snapshots.
- `--mode eac`: execute only after consensus ordering.
- `--mode no-snapshots`: speculate without snapshots.
- `--mode eager-snapshots`: speculate with eager snapshots.
- `--workload erc20` (default), `weth`, or `uniswap`.

The orchestrator selects the matching `5_5_8` storage and account-address artifacts and includes
both mode and workload in result and log names. For example:

```bash
cargo run --release --bin orchestrator -- \
  benchmark --committee 10 --loads 400 \
  --mode eager-snapshots --workload uniswap
```

When monitoring is enabled, each AWS node also samples the validator process once per second and
exports `mysticeti_process_cpu_percent` and `mysticeti_process_resident_memory_bytes` through
node-exporter. The default Grafana dashboard shows **Validator CPU (%)** and **Validator RSS (MB)**
immediately below the latency panel. CPU follows `ps` semantics and can exceed 100% when the
validator uses more than one CPU core; RSS is the validator process's resident memory, not total
host memory.

For a fixed interval, the same configuration can be placed in `node-parameters.yml` instead:

```yaml
direct_commit_stall_simulation:
  start_time:
    secs: 10
    nanos: 0
  duration:
    secs: 35
    nanos: 0
```

Command-line stall arguments override the interval loaded from the node-parameters file.

## Experiment Modes

Both `speculative.sh` and `jitterrun.sh` accept an optional experiment mode, workload, and output directory:

```bash
SKIP_BUILD=1 bash speculative.sh 4 60 100 full uniswap ./results/full-dryrun
SKIP_BUILD=1 bash jitterrun.sh 90 7 1 4 2500 10 50 100 full weth ./results/full-jitter
```

Supported modes:

- `full`: the full speculative design
- `eac`: execution-after-consensus only
- `no-aps`: speculative execution with naive all-commit prediction
- `all-skip`: speculative execution with a naive always-skip prediction baseline
- `no-snapshots`: speculative execution without rollback snapshots
- `eager-snapshots`: speculative execution with eager snapshotting

These modes are intended for ablation studies.

Supported workloads:

- `erc20`: baseline ERC-20 transfer workload (simple token transfer)
- `weth`: WETH9 deposit / approve / transferFrom / withdraw mix (a mixed workload of deposit, approve, transferFrom, and withdraw)
- `uniswap`: Uniswap V3 single-swap workload (swap transactions through a Uniswap-V3-style single-swap contract)

## Ablation Sweep

Run the full ablation matrix:

```bash
SKIP_BUILD=1 bash ablation_study.sh jitter ./results/ablation-jitter erc20
```

For a no-jitter sweep:

```bash
SKIP_BUILD=1 bash ablation_study.sh dryrun ./results/ablation-dryrun uniswap
```

By default the sweep covers:

- `full`
- `eac`
- `no-aps`
- `no-snapshots`
- `eager-snapshots`

You can override the set with `MODES="..."`.

Arguments:

1. experiment kind: `jitter` or `dryrun`
2. root output directory
3. workload: `erc20`, `weth`, or `uniswap`

The sweep writes results under `<root>/<workload>/<mode>/`.

## APS Ablation

Run the APS-focused jitter ablation sweep:

```bash
SKIP_BUILD=1 bash scripts/ablation_aps.sh 3
```

Argument:

1. `REPEAT`: number of times to repeat the full sweep

This script evaluates `full`, `no-aps`, and `all-skip` on `erc20` with:

- `COMMITTEE_SIZE in {10, 30}`
- `FAULT_NUM = DELAY_CONNECTION_NUM in {30%, 50% of committee size}`
- `LOAD=100`
- `TOTAL_DURATION=300`
- `JITTER_MS=1500`
- `JITTER_START_TIME=1`
- `JITTER_DURATION=300`

Each run is written to:

- `./results/ablation-aps-<mode>-<committee_size>-<fault_num>-<timestamp>/`

## Snapshot Ablation

Run the snapshot-policy ablation sweep:

```bash
SKIP_BUILD=1 bash scripts/ablation_snapshot.sh 3
```

Argument:

1. `REPEAT`: number of times to repeat the full sweep

This script evaluates `full`, `no-snapshots`, and `eager-snapshots` on `erc20` with:

- speculative runs for `COMMITTEE_SIZE in {10, 30}`, `LOAD=100`, `TOTAL_DURATION=300`
- jitter runs for `COMMITTEE_SIZE in {10, 30}`, `FAULT_NUM = DELAY_CONNECTION_NUM = 50% of committee size`, `LOAD=100`, `TOTAL_DURATION=300`, `JITTER_MS=1500`, `JITTER_START_TIME=1`, `JITTER_DURATION=300`

Each run is written to:

- `./results/ablation-snapshot-speculative-<mode>-<committee_size>-<timestamp>/`
- `./results/ablation-snapshot-jitter-<mode>-<committee_size>-<fault_num>-<timestamp>/`

## Snapshot Attack Ablation

Run the snapshot-policy ablation with consecutive direct-commit stalls instead of network jitter:

```bash
SKIP_BUILD=1 bash scripts/ablation_snapshot_attack.sh 3
```

The positional argument is `REPEAT`, the number of times to repeat the complete sweep. For every
snapshot policy and committee size, the script runs both a normal speculative control and a
commit-stall attack. The default matrix is:

- modes: `full`, `no-snapshots`, and `eager-snapshots`
- committee sizes: `10` and `30`
- workload: `erc20`
- load: `100`
- total duration: `300` seconds
- attack interval: `[1, 250)` seconds, leaving 50 seconds to measure catch-up

Experiment parameters can be overridden through environment variables:

```bash
SKIP_BUILD=1 \
TOTAL_DURATION=300 \
STALL_START=10 \
STALL_END=280 \
LOAD=100 \
WORKLOAD=erc20 \
RESOURCE_MONITOR_INTERVAL=1 \
COMMITTEE_SIZES="10 30" \
MODES="full no-snapshots eager-snapshots" \
bash scripts/ablation_snapshot_attack.sh 3
```

`TOTAL_DURATION` must be greater than `STALL_END`, and `STALL_END` must be greater than
`STALL_START`. Results are organized as:

- `./results/ablation-snapshot-attack/speculative/<mode>-<committee_size>-<timestamp>/`
- `./results/ablation-snapshot-attack/attack/<mode>-<committee_size>-<timestamp>/`

Each leaf directory contains validator logs, raw metrics, `run-meta.txt`, and `summary.csv` in the
same format as the other experiment scripts. It also samples each validator's resource usage every
`RESOURCE_MONITOR_INTERVAL` seconds (one second by default) and writes:

- `resource-usage.csv`: timestamped per-validator CPU percentage and resident memory (RSS in KiB)
- `resource-summary.csv`: average and peak CPU/RSS for each validator and the aggregate committee
- `validator-<authority>.pid`: the validator PID used by the resource sampler

In `resource-summary.csv`, the `all` row is the sum across validators at each sample time. Its CPU
percentage can exceed 100% when validators use multiple CPU cores.

## Ablation Result Summaries

Run the summarizers from the `mysticeti/scripts` directory:

```bash
cd mysticeti/scripts
python3 summarize_ablation_aps.py
python3 summarize_ablation_snapshot.py
python3 summarize_ablation_snapshot_attack.py
```

The scripts group timestamped directories with the same experiment parameters and average repeated
runs. The APS summarizer reports prediction hit rate and p99 transaction commit latency. The
snapshot and snapshot-attack summarizers report snapshot-store size, p99 transaction commit
latency, average CPU utilization, and average RSS memory. CPU and RSS are averaged across
authorities; the aggregate `all` row is excluded. Each script accepts an optional result root as
its first argument when the default directory is not being used.

## Microbenchmark Sweep

Run a simple load sweep for a chosen mode:

```bash
SKIP_BUILD=1 MODE=full SWEEP_LOADS="25 50 75 100 125" bash scripts/microbench.sh ./results/microbench-load
```

This repeatedly invokes `jitterrun.sh` while varying the offered load.

## Output Format

Each run directory contains:

- `validator-*.metrics`: scraped Prometheus metrics for each validator
- `validator-*.pid`: validator process IDs used for resource monitoring
- `v*.log.ansi`: validator logs
- `run-meta.txt`: run configuration
- `summary.csv`: condensed metrics generated by [`scripts/summarize_metrics.py`](scripts/summarize_metrics.py)
- `resource-usage.csv`: per-validator CPU and RSS time series
- `resource-summary.csv`: per-validator and committee-wide average/peak CPU and RSS

To summarize a parent directory containing repeated runs with the same parameters:

```bash
python3 scripts/summarize_metrics.py ./results > ./results/summary.csv
```

When multiple run directories share the same parameters, the summarizer emits additional rows with `validator=repeat_average` so repeated experiments can be compared in the same CSV file.

The summary currently extracts the most useful paper-facing metrics, including:

- `transaction_committed_latency`
- `block_execution_latency`
- `block_consensus_latency`
- `submitted_transactions`
- `speculative_messages_total`
- `speculative_predictions_total`
- `speculative_snapshot_total`
- `speculative_execution_leaders_total`
- `speculative_prefix_matched_leaders_total`
- `speculative_reexecuted_leaders_total`
- `speculative_snapshot_window_size`
- `speculative_snapshot_non_window_peak_size`
- `speculative_snapshot_store_size`

Example raw metrics:

```text
# speculative_predictions_total is used to calculate the prediction accuracy
speculative_predictions_total{outcome="commit_hit"} 378
speculative_predictions_total{outcome="skip_hit"} 23
speculative_predictions_total{outcome="skip_miss"} 5

# speculative_snapshot_total{kind="pre_exec"} is the number of snapshots a node took, with
speculative_snapshot_total{kind="pre_exec"} 2
speculative_snapshot_total{kind="reuse"} 480

block_execution_latency{v="count"} 6570
block_execution_latency{v="p50"} 530
block_execution_latency{v="p90"} 2848
block_execution_latency{v="p99"} 5233
block_execution_latency{v="sum"} 6989851

transaction_committed_latency{v="count"} 47200
transaction_committed_latency{v="p50"} 62850
transaction_committed_latency{v="p90"} 82400
transaction_committed_latency{v="p99"} 752120
transaction_committed_latency{v="sum"} 3690243727
```

## Notes

- The wrappers use `tmux` and expect it to be available locally.
- The current scripts are designed for local experimentation and paper evaluation, not production deployment.
- `cargo fmt --all` may also reformat files in the sibling `pevm` dependency if both live in the same workspace checkout.

## Evaluation on AutobahnEVM

Please see [AutobahnEVM](https://anonymous.4open.science/r/AutobahnEVM-C5BA/README.md).

## Evaluation on AWS

Will add detailed instructions after the paper is public.

## License

This software is licensed as [Apache 2.0](LICENSE).
