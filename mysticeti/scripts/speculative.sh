#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CALLER_PWD="$(pwd)"
BIN_PATH="${PROJECT_ROOT}/target/debug/mysticeti"
source "${SCRIPT_DIR}/validator_lifecycle.sh"

COMMITTEE_SIZE=${1:-4}
DURATION=${2:-15}
LOAD=${3:-100}
EXPERIMENT_MODE=${4:-full}
WORKLOAD=${5:-erc20}
OUTPUT_DIR=${6:-"./results/speculative-${EXPERIMENT_MODE}-${WORKLOAD}-$(date +%Y%m%d-%H%M%S)"}
SKIP_BUILD=${SKIP_BUILD:-0}
RESOURCE_MONITOR_INTERVAL=${RESOURCE_MONITOR_INTERVAL:-1}
RESOURCE_MONITOR_PID=""

if [[ "${OUTPUT_DIR}" != /* ]]; then
    OUTPUT_DIR="${CALLER_PWD}/${OUTPUT_DIR}"
fi

cd "${PROJECT_ROOT}"

if [ "${SKIP_BUILD}" != "1" ] || [ ! -x "${BIN_PATH}" ]; then
    cargo build 2>&1 >/dev/null | tail -n 10
fi

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::speculative_executor=DEBUG,mysticeti_core::block_handler=INFO,

mkdir -p "${OUTPUT_DIR}"
tmux kill-server 2>/dev/null || true
stop_stale_validators

cleanup() {
    if [[ -n "${RESOURCE_MONITOR_PID}" ]]; then
        kill "${RESOURCE_MONITOR_PID}" 2>/dev/null || true
        wait "${RESOURCE_MONITOR_PID}" 2>/dev/null || true
    fi
    stop_registered_validators
}
trap cleanup EXIT

echo "Starting validators in mode=${EXPERIMENT_MODE} workload=${WORKLOAD}..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cd ${PROJECT_ROOT} && exec ${BIN_PATH} dry-run --committee-size ${COMMITTEE_SIZE} --authority ${i} --load ${LOAD} --experiment-mode ${EXPERIMENT_MODE} --workload ${WORKLOAD} > ${OUTPUT_DIR}/v${i}.log.ansi 2>&1"
    validator_pid=$(tmux display-message -p -t "v${i}:0.0" '#{pane_pid}')
    printf '%s\n' "${validator_pid}" > "${OUTPUT_DIR}/validator-${i}.pid"
    register_validator_pid "${validator_pid}"
done

bash "${SCRIPT_DIR}/monitor_resources.sh" \
    "${OUTPUT_DIR}" "${COMMITTEE_SIZE}" "${RESOURCE_MONITOR_INTERVAL}" &
RESOURCE_MONITOR_PID=$!

sleep "${DURATION}"

kill "${RESOURCE_MONITOR_PID}" 2>/dev/null || true
wait "${RESOURCE_MONITOR_PID}" 2>/dev/null || true
RESOURCE_MONITOR_PID=""

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    curl -s "http://0.0.0.0:$((1500 + COMMITTEE_SIZE + i))/metrics" > "${OUTPUT_DIR}/validator-${i}.metrics"
done

cat > "${OUTPUT_DIR}/run-meta.txt" <<EOF
experiment=speculative
committee_size=${COMMITTEE_SIZE}
duration=${DURATION}
load=${LOAD}
experiment_mode=${EXPERIMENT_MODE}
workload=${WORKLOAD}
resource_monitor_interval=${RESOURCE_MONITOR_INTERVAL}
EOF

if [ -f "${SCRIPT_DIR}/summarize_metrics.py" ]; then
    python3 "${SCRIPT_DIR}/summarize_metrics.py" "${OUTPUT_DIR}" > "${OUTPUT_DIR}/summary.csv"
fi

echo "Results written to ${OUTPUT_DIR}"
