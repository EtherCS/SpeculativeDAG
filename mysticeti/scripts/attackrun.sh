#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CALLER_PWD="$(pwd)"
BIN_PATH="${PROJECT_ROOT}/target/debug/mysticeti"
source "${SCRIPT_DIR}/validator_lifecycle.sh"

DURATION=${1:-90}
COMMITTEE_SIZE=${2:-7}
STALL_START=${3:-10}
STALL_END=${4:-60}
LOAD=${5:-100}
EXPERIMENT_MODE=${6:-full}
WORKLOAD=${7:-erc20}
OUTPUT_DIR=${8:-"./results/attack-${EXPERIMENT_MODE}-${WORKLOAD}-${COMMITTEE_SIZE}-$(date +%Y%m%d-%H%M%S)"}
SKIP_BUILD=${SKIP_BUILD:-0}
RESOURCE_MONITOR_INTERVAL=${RESOURCE_MONITOR_INTERVAL:-1}
PREDICTION_ERROR_RATE=${PREDICTION_ERROR_RATE:-0}
PREDICTION_ERROR_SEED=${PREDICTION_ERROR_SEED:-0}
RESOURCE_MONITOR_PID=""

if (( STALL_END < STALL_START )); then
    echo "STALL_END must be greater than or equal to STALL_START" >&2
    exit 2
fi
if (( DURATION <= STALL_END )); then
    echo "DURATION must be greater than STALL_END so recovery can be measured" >&2
    exit 2
fi
if (( PREDICTION_ERROR_RATE < 0 || PREDICTION_ERROR_RATE > 100 )); then
    echo "PREDICTION_ERROR_RATE must be between 0 and 100" >&2
    exit 2
fi
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

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::core=debug,mysticeti_core::speculative_executor=debug,mysticeti_core::executor=info,pevm=info

mkdir -p "${OUTPUT_DIR}"
stop_stale_validators
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

echo "Starting attack run in mode=${EXPERIMENT_MODE} workload=${WORKLOAD} stall=[${STALL_START},${STALL_END})..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cd ${PROJECT_ROOT} && exec ${BIN_PATH} attack-run --authority ${i} --committee-size ${COMMITTEE_SIZE} --stall-start ${STALL_START} --stall-end ${STALL_END} --load ${LOAD} --experiment-mode ${EXPERIMENT_MODE} --workload ${WORKLOAD} --prediction-error-rate ${PREDICTION_ERROR_RATE} --prediction-error-seed ${PREDICTION_ERROR_SEED} > ${OUTPUT_DIR}/v${i}.log.ansi 2>&1"
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
experiment=direct-commit-stall
duration=${DURATION}
committee_size=${COMMITTEE_SIZE}
stall_start=${STALL_START}
stall_end=${STALL_END}
load=${LOAD}
experiment_mode=${EXPERIMENT_MODE}
workload=${WORKLOAD}
resource_monitor_interval=${RESOURCE_MONITOR_INTERVAL}
prediction_error_rate=${PREDICTION_ERROR_RATE}
prediction_error_seed=${PREDICTION_ERROR_SEED}
EOF

if [ -f "${SCRIPT_DIR}/summarize_metrics.py" ]; then
    python3 "${SCRIPT_DIR}/summarize_metrics.py" "${OUTPUT_DIR}" > "${OUTPUT_DIR}/summary.csv"
fi

echo "Results written to ${OUTPUT_DIR}"
