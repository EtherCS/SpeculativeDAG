#!/bin/bash

set -euo pipefail

COMMITTEE_SIZE=${1:-4}
DURATION=${2:-15}
EXPERIMENT_MODE=${3:-full}
OUTPUT_DIR=${4:-"./results/speculative-${EXPERIMENT_MODE}-$(date +%Y%m%d-%H%M%S)"}
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

echo "Starting validators in mode=${EXPERIMENT_MODE}..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cargo run --bin mysticeti -- dry-run --committee-size ${COMMITTEE_SIZE} --authority ${i} --experiment-mode ${EXPERIMENT_MODE} > ${OUTPUT_DIR}/v${i}.log.ansi"
done

sleep "${DURATION}"

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    curl -s "http://0.0.0.0:$((1500 + COMMITTEE_SIZE + i))/metrics" > "${OUTPUT_DIR}/validator-${i}.metrics"
done

cat > "${OUTPUT_DIR}/run-meta.txt" <<EOF
experiment=speculative
committee_size=${COMMITTEE_SIZE}
duration=${DURATION}
experiment_mode=${EXPERIMENT_MODE}
EOF

if [ -f scripts/summarize_metrics.py ]; then
    python3 scripts/summarize_metrics.py "${OUTPUT_DIR}" > "${OUTPUT_DIR}/summary.csv"
fi

echo "Results written to ${OUTPUT_DIR}"
