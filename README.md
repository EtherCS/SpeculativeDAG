# Pufferfish

[![build status](https://img.shields.io/github/actions/workflow/status/asonnino/shamir-bip39/code.yml?branch=main&logo=github&style=flat-square)](https://github.com/asonnino/shamir-bip39/actions)
[![rustc](https://img.shields.io/badge/rustc-1.78+-blue?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-Apache-blue.svg?style=flat-square)](LICENSE)

The code in this branch is a prototype of Pufferfish. It supplements the paper [Masking Ordering Failures in BFT SMR via DAG-based Proactive Pre-Commit Execution](https://eprint.iacr.org/2026/796.pdf) enabling reproducible results. There are no plans to maintain this branch.

This fork also contains an experimental prototype for speculative pre-commit execution, together with local scripts for latency experiments, ablation studies, and microbenchmarks.

## Build

```bash
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

## Microbenchmark Sweep

Run a simple load sweep for a chosen mode:

```bash
SKIP_BUILD=1 MODE=full SWEEP_LOADS="25 50 75 100 125" bash scripts/microbench.sh ./results/microbench-load
```

This repeatedly invokes `jitterrun.sh` while varying the offered load.

## Output Format

Each run directory contains:

- `validator-*.metrics`: scraped Prometheus metrics for each validator
- `v*.log.ansi`: validator logs
- `run-meta.txt`: run configuration
- `summary.csv`: condensed metrics generated by [`scripts/summarize_metrics.py`](scripts/summarize_metrics.py)

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

## License

This software is licensed as [Apache 2.0](LICENSE).
