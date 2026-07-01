#!/bin/bash

set -euo pipefail

DURATION=${1:-90}
COMMITTEE_SIZE=${2:-7}
FAULT_NUM=${3:-1}
DELAY_CONNECTION_NUM=${4:-4}
JITTER_MS=${5:-2500}
JITTER_START_TIME=${6:-10}
JITTER_DURATION=${7:-50}
LOAD=${8:-100}
EXPERIMENT_MODE=${9:-full}
OUTPUT_DIR=${10:-"./results/jitter-${EXPERIMENT_MODE}-$(date +%Y%m%d-%H%M%S)"}
SKIP_BUILD=${SKIP_BUILD:-0}

if [ "${SKIP_BUILD}" != "1" ]; then
    cargo build 2>&1 >/dev/null | tail -n 10
fi

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::speculative_executor=DEBUG,mysticeti_core::block_handler=INFO,

mkdir -p "${OUTPUT_DIR}"
tmux kill-server || true

cleanup() {
    tmux kill-server || true
}
trap cleanup EXIT

echo "Starting validators in mode=${EXPERIMENT_MODE} with network jitter..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cargo run --bin mysticeti -- jitter-run --authority ${i} --committee-size ${COMMITTEE_SIZE} --fault-num ${FAULT_NUM} --delay-connection-num ${DELAY_CONNECTION_NUM} --jitter-ms ${JITTER_MS} --start-time ${JITTER_START_TIME} --duration-secs ${JITTER_DURATION} --load ${LOAD} --experiment-mode ${EXPERIMENT_MODE} > ${OUTPUT_DIR}/v${i}.log.ansi"
done

sleep "${DURATION}"

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    curl -s "http://0.0.0.0:$((1500 + COMMITTEE_SIZE + i))/metrics" > "${OUTPUT_DIR}/validator-${i}.metrics"
done

cat > "${OUTPUT_DIR}/run-meta.txt" <<EOF
experiment=jitter
duration=${DURATION}
committee_size=${COMMITTEE_SIZE}
fault_num=${FAULT_NUM}
delay_connection_num=${DELAY_CONNECTION_NUM}
jitter_ms=${JITTER_MS}
jitter_start_time=${JITTER_START_TIME}
jitter_duration=${JITTER_DURATION}
load=${LOAD}
experiment_mode=${EXPERIMENT_MODE}
EOF

if [ -f scripts/summarize_metrics.py ]; then
    python3 scripts/summarize_metrics.py "${OUTPUT_DIR}" > "${OUTPUT_DIR}/summary.csv"
fi

echo "Results written to ${OUTPUT_DIR}"
