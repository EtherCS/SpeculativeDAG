#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"

REPEAT="${1:-${REPEAT:-1}}"
OUTPUT_ROOT="${2:-${OUTPUT_ROOT:-./results/load-stall-sweep}}"
COMMITTEE_SIZE="${COMMITTEE_SIZE:-7}"
LOADS="${LOADS:-25 50 75 100 125}"
STALL_DURATIONS="${STALL_DURATIONS:-0 5 10 20 40}"
MODES="${MODES:-full eac}"
STALL_START="${STALL_START:-10}"
RECOVERY_DURATION="${RECOVERY_DURATION:-30}"
WORKLOAD="${WORKLOAD:-erc20}"

if [[ "${OUTPUT_ROOT}" != /* ]]; then
    OUTPUT_ROOT="${CALLER_PWD}/${OUTPUT_ROOT}"
fi
mkdir -p "${OUTPUT_ROOT}"

for repeat in $(seq 1 "${REPEAT}"); do
    for mode in ${MODES}; do
        for load in ${LOADS}; do
            for stall_duration in ${STALL_DURATIONS}; do
                run_dir="${OUTPUT_ROOT}/${mode}/load-${load}/stall-${stall_duration}/repeat-${repeat}"
                if [[ -s "${run_dir}/run-meta.txt" && -s "${run_dir}/summary.csv" ]]; then
                    echo "Skipping completed run: ${run_dir}"
                    continue
                fi

                mkdir -p "${run_dir}"
                total_duration=$((STALL_START + stall_duration + RECOVERY_DURATION))
                echo "Running mode=${mode} load=${load} stall=${stall_duration}s repeat=${repeat}/${REPEAT}"

                if (( stall_duration == 0 )); then
                    SKIP_BUILD="${SKIP_BUILD:-0}" bash "${SCRIPT_DIR}/speculative.sh" "${COMMITTEE_SIZE}" "${total_duration}" "${load}" "${mode}" "${WORKLOAD}" "${run_dir}"
                else
                    stall_end=$((STALL_START + stall_duration))
                    SKIP_BUILD="${SKIP_BUILD:-0}" bash "${SCRIPT_DIR}/attackrun.sh" "${total_duration}" "${COMMITTEE_SIZE}" "${STALL_START}" "${stall_end}" "${load}" "${mode}" "${WORKLOAD}" "${run_dir}"
                fi
            done
        done
    done
done

python3 "${SCRIPT_DIR}/summarize_load_stall_sweep.py" "${OUTPUT_ROOT}" --csv "${OUTPUT_ROOT}/load-stall-summary.csv" --figure "${OUTPUT_ROOT}/load-stall-heatmap.pdf"

echo "Sweep complete: ${OUTPUT_ROOT}"
