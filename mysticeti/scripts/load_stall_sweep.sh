#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"

REPEAT="${1:-${REPEAT:-1}}"
OUTPUT_ROOT="${2:-${OUTPUT_ROOT:-./results/load-stall-sweep}}"
COMMITTEE_SIZE="${COMMITTEE_SIZE:-10}"
LOADS="${LOADS:-25 50 75 100}"
STALL_DURATIONS="${STALL_DURATIONS:-0 5 10 20 40}"
MODES="${MODES:-full eac}"
STALL_START="${STALL_START:-10}"
RECOVERY_DURATION="${RECOVERY_DURATION:-30}"
WORKLOAD="${WORKLOAD:-erc20}"

if [[ "${OUTPUT_ROOT}" != /* ]]; then
    OUTPUT_ROOT="${CALLER_PWD}/${OUTPUT_ROOT}"
fi
mkdir -p "${OUTPUT_ROOT}"

run_is_complete() {
    local run_dir=$1
    local validator metrics_file

    [[ -s "${run_dir}/run-meta.txt" && -s "${run_dir}/summary.csv" ]] || return 1
    for validator in $(seq 0 $((COMMITTEE_SIZE - 1))); do
        metrics_file="${run_dir}/validator-${validator}.metrics"
        [[ -s "${metrics_file}" ]] || return 1
        awk '
            $1 == "boundary_transaction_commit_latency_us" && ($2 + 0) > 0 { found = 1 }
            END { exit(found ? 0 : 1) }
        ' "${metrics_file}" || return 1
    done
}

for repeat in $(seq 1 "${REPEAT}"); do
    for mode in ${MODES}; do
        for load in ${LOADS}; do
            for stall_duration in ${STALL_DURATIONS}; do
                run_dir="${OUTPUT_ROOT}/${mode}/load-${load}/stall-${stall_duration}/repeat-${repeat}"
                if run_is_complete "${run_dir}"; then
                    echo "Skipping completed run: ${run_dir}"
                    continue
                fi

                mkdir -p "${run_dir}"
                total_duration=$((STALL_START + stall_duration + RECOVERY_DURATION))
                echo "Running mode=${mode} load=${load} stall=${stall_duration}s repeat=${repeat}/${REPEAT}"

                stall_end=$((STALL_START + stall_duration))
                SKIP_BUILD="${SKIP_BUILD:-0}" bash "${SCRIPT_DIR}/attackrun.sh" "${total_duration}" "${COMMITTEE_SIZE}" "${STALL_START}" "${stall_end}" "${load}" "${mode}" "${WORKLOAD}" "${run_dir}"
            done
        done
    done
done

python3 "${SCRIPT_DIR}/summarize_load_stall_sweep.py" "${OUTPUT_ROOT}" --csv "${OUTPUT_ROOT}/load-stall-summary.csv" --figure "${OUTPUT_ROOT}/load-stall-heatmap.pdf"

echo "Sweep complete: ${OUTPUT_ROOT}"
