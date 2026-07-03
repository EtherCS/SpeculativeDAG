#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CALLER_PWD="$(pwd)"
BIN_PATH="${PROJECT_ROOT}/target/debug/mysticeti"

COMMITTEE_SIZE=${1:-4}
DURATION=${2:-15}
EXPERIMENT_MODE=${3:-full}
WORKLOAD=${4:-erc20}
OUTPUT_DIR=${5:-"./results/speculative-${EXPERIMENT_MODE}-${WORKLOAD}-$(date +%Y%m%d-%H%M%S)"}
SKIP_BUILD=${SKIP_BUILD:-0}

if [[ "${OUTPUT_DIR}" != /* ]]; then
    OUTPUT_DIR="${CALLER_PWD}/${OUTPUT_DIR}"
fi

cd "${PROJECT_ROOT}"

if [ "${SKIP_BUILD}" != "1" ] || [ ! -x "${BIN_PATH}" ]; then
    cargo build 2>&1 >/dev/null | tail -n 10
fi

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::speculative_executor=DEBUG,mysticeti_core::block_handler=INFO,

mkdir -p "${OUTPUT_DIR}"
tmux kill-server || true

cleanup() {
    tmux kill-server || true
}
trap cleanup EXIT

echo "Starting validators in mode=${EXPERIMENT_MODE} workload=${WORKLOAD}..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cd ${PROJECT_ROOT} && ${BIN_PATH} dry-run --committee-size ${COMMITTEE_SIZE} --authority ${i} --experiment-mode ${EXPERIMENT_MODE} --workload ${WORKLOAD} > ${OUTPUT_DIR}/v${i}.log.ansi 2>&1"
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
workload=${WORKLOAD}
EOF

if [ -f "${SCRIPT_DIR}/summarize_metrics.py" ]; then
    python3 "${SCRIPT_DIR}/summarize_metrics.py" "${OUTPUT_DIR}" > "${OUTPUT_DIR}/summary.csv"
fi

echo "Results written to ${OUTPUT_DIR}"
