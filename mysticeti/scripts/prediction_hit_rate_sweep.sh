#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"

REPEAT="${1:-${REPEAT:-1}}"
OUTPUT_ROOT="${2:-${OUTPUT_ROOT:-./results/prediction-hit-rate-sweep}}"
ERROR_RATES="${ERROR_RATES:-0 10 25 50 75 100}"
COMMITTEE_SIZE="${COMMITTEE_SIZE:-7}"
LOAD="${LOAD:-100}"
STALL_START="${STALL_START:-10}"
STALL_DURATION="${STALL_DURATION:-20}"
RECOVERY_DURATION="${RECOVERY_DURATION:-30}"
WORKLOAD="${WORKLOAD:-erc20}"
BASE_SEED="${BASE_SEED:-2027}"

if [[ "${OUTPUT_ROOT}" != /* ]]; then
    OUTPUT_ROOT="${CALLER_PWD}/${OUTPUT_ROOT}"
fi
mkdir -p "${OUTPUT_ROOT}"

stall_end=$((STALL_START + STALL_DURATION))
total_duration=$((stall_end + RECOVERY_DURATION))

for repeat in $(seq 1 "${REPEAT}"); do
    eac_dir="${OUTPUT_ROOT}/eac/repeat-${repeat}"
    if [[ ! -s "${eac_dir}/run-meta.txt" || ! -s "${eac_dir}/summary.csv" ]]; then
        mkdir -p "${eac_dir}"
        echo "Running EAC reference repeat=${repeat}/${REPEAT}"
        SKIP_BUILD="${SKIP_BUILD:-0}" bash "${SCRIPT_DIR}/attackrun.sh" "${total_duration}" "${COMMITTEE_SIZE}" "${STALL_START}" "${stall_end}" "${LOAD}" eac "${WORKLOAD}" "${eac_dir}"
    fi

    for error_rate in ${ERROR_RATES}; do
        run_dir="${OUTPUT_ROOT}/error-${error_rate}/repeat-${repeat}"
        if [[ -s "${run_dir}/run-meta.txt" && -s "${run_dir}/summary.csv" ]]; then
            echo "Skipping completed run: ${run_dir}"
            continue
        fi

        seed=$((BASE_SEED + repeat - 1))
        mkdir -p "${run_dir}"
        echo "Running requested-error=${error_rate}% seed=${seed} repeat=${repeat}/${REPEAT}"
        PREDICTION_ERROR_RATE="${error_rate}" PREDICTION_ERROR_SEED="${seed}" SKIP_BUILD="${SKIP_BUILD:-0}" bash "${SCRIPT_DIR}/attackrun.sh" "${total_duration}" "${COMMITTEE_SIZE}" "${STALL_START}" "${stall_end}" "${LOAD}" full "${WORKLOAD}" "${run_dir}"
    done
done

python3 "${SCRIPT_DIR}/summarize_prediction_hit_rate_sweep.py" "${OUTPUT_ROOT}" --csv "${OUTPUT_ROOT}/prediction-hit-rate-summary.csv" --figure "${OUTPUT_ROOT}/prediction-hit-rate-sweep.pdf"

echo "Sweep complete: ${OUTPUT_ROOT}"
