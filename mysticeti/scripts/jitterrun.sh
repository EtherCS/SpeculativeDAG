#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CALLER_PWD="$(pwd)"
BIN_PATH="${PROJECT_ROOT}/target/debug/mysticeti"
source "${SCRIPT_DIR}/validator_lifecycle.sh"

DURATION=${1:-90}
COMMITTEE_SIZE=${2:-7}
FAULT_NUM=${3:-1}
DELAY_CONNECTION_NUM=${4:-4}
JITTER_MS=${5:-2500}
JITTER_START_TIME=${6:-10}
JITTER_DURATION=${7:-50}
LOAD=${8:-100}
EXPERIMENT_MODE=${9:-full}
WORKLOAD=${10:-erc20}
OUTPUT_DIR=${11:-"./results/jitter-${EXPERIMENT_MODE}-${WORKLOAD}-$(date +%Y%m%d-%H%M%S)"}
SKIP_BUILD=${SKIP_BUILD:-0}
RESOURCE_MONITOR_INTERVAL=${RESOURCE_MONITOR_INTERVAL:-1}
RESOURCE_MONITOR_PID=""

if [[ "${OUTPUT_DIR}" != /* ]]; then
    OUTPUT_DIR="${CALLER_PWD}/${OUTPUT_DIR}"
fi

cd "${PROJECT_ROOT}"

NEWER_SOURCE=""
if [ -x "${BIN_PATH}" ]; then
    NEWER_SOURCE=$(find "${PROJECT_ROOT}/crates" -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -newer "${BIN_PATH}" -print -quit)
fi
if [ "${SKIP_BUILD}" != "1" ] || [ ! -x "${BIN_PATH}" ] || [ -n "${NEWER_SOURCE}" ]; then
    if [ -n "${NEWER_SOURCE}" ]; then
        echo "Source changes detected; rebuilding ${BIN_PATH} despite SKIP_BUILD=1"
    fi
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

echo "Starting validators in mode=${EXPERIMENT_MODE} workload=${WORKLOAD} with network jitter..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cd ${PROJECT_ROOT} && exec ${BIN_PATH} jitter-run --authority ${i} --committee-size ${COMMITTEE_SIZE} --fault-num ${FAULT_NUM} --delay-connection-num ${DELAY_CONNECTION_NUM} --jitter-ms ${JITTER_MS} --start-time ${JITTER_START_TIME} --duration-secs ${JITTER_DURATION} --load ${LOAD} --experiment-mode ${EXPERIMENT_MODE} --workload ${WORKLOAD} > ${OUTPUT_DIR}/v${i}.log.ansi 2>&1"
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
workload=${WORKLOAD}
resource_monitor_interval=${RESOURCE_MONITOR_INTERVAL}
EOF

if [ -f "${SCRIPT_DIR}/summarize_metrics.py" ]; then
    python3 "${SCRIPT_DIR}/summarize_metrics.py" "${OUTPUT_DIR}" > "${OUTPUT_DIR}/summary.csv"
fi

echo "Results written to ${OUTPUT_DIR}"
