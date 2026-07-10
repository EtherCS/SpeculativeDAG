#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"
REPEAT=${1:-1}
WORKLOAD="erc20"
LOAD=100
TOTAL_DURATION=300
JITTER_MS=1500
JITTER_START_TIME=1
JITTER_DURATION=300
COMMITTEE_SIZES=(10 30)
MODES=(full no-aps all-skip)

resolve_output_dir() {
    local path="$1"
    if [[ "${path}" = /* ]]; then
        printf '%s\n' "${path}"
    else
        printf '%s\n' "${CALLER_PWD}/${path}"
    fi
}

for run_id in $(seq 1 "${REPEAT}"); do
    timestamp="$(date +%Y%m%d-%H%M%S)"
    for mode in "${MODES[@]}"; do
        for committee_size in "${COMMITTEE_SIZES[@]}"; do
            speculative_root=$(resolve_output_dir "./results/ablation-aps/speculative/${mode}-${committee_size}-${timestamp}")
            mkdir -p "${speculative_root}"

            echo "Running APS ablation (speculative): repeat=${run_id}/${REPEAT} mode=${mode} committee_size=${committee_size}"
            bash "${SCRIPT_DIR}/speculative.sh" \
                "${committee_size}" \
                "${TOTAL_DURATION}" \
                "${LOAD}" \
                "${mode}" \
                "${WORKLOAD}" \
                "${speculative_root}"
            sleep 1

            fault_counts=( $((committee_size * 30 / 100)) $((committee_size * 50 / 100)) )
            for fault_num in "${fault_counts[@]}"; do
                jitter_dir=$(resolve_output_dir "./results/ablation-aps/jitter/${mode}-${committee_size}-${fault_num}-${timestamp}")
                mkdir -p "${jitter_dir}"

                echo "Running APS ablation (jitter): repeat=${run_id}/${REPEAT} mode=${mode} committee_size=${committee_size} fault_num=${fault_num}"
                bash "${SCRIPT_DIR}/jitterrun.sh" \
                    "${TOTAL_DURATION}" \
                    "${committee_size}" \
                    "${fault_num}" \
                    "${fault_num}" \
                    "${JITTER_MS}" \
                    "${JITTER_START_TIME}" \
                    "${JITTER_DURATION}" \
                    "${LOAD}" \
                    "${mode}" \
                    "${WORKLOAD}" \
                    "${jitter_dir}"
                sleep 1
            done
        done
    done
done

echo "APS ablation complete"
