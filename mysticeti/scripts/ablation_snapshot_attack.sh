#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"
REPEAT=${1:-1}
WORKLOAD="${WORKLOAD:-erc20}"
LOAD=${LOAD:-100}
TOTAL_DURATION=${TOTAL_DURATION:-300}
STALL_START=${STALL_START:-1}
STALL_END=${STALL_END:-250}
COMMITTEE_SIZES=(${COMMITTEE_SIZES:-10 30})
MODES=(${MODES:-full no-snapshots eager-snapshots})

if (( STALL_END <= STALL_START )); then
    echo "STALL_END must be greater than STALL_START" >&2
    exit 2
fi
if (( TOTAL_DURATION <= STALL_END )); then
    echo "TOTAL_DURATION must be greater than STALL_END so recovery can be measured" >&2
    exit 2
fi

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
            speculative_root=$(resolve_output_dir "./results/ablation-snapshot-attack/speculative/${mode}-${committee_size}-${timestamp}")
            mkdir -p "${speculative_root}"

            echo "Running snapshot ablation (speculative): repeat=${run_id}/${REPEAT} mode=${mode} committee_size=${committee_size}"
            bash "${SCRIPT_DIR}/speculative.sh" \
                "${committee_size}" \
                "${TOTAL_DURATION}" \
                "${LOAD}" \
                "${mode}" \
                "${WORKLOAD}" \
                "${speculative_root}"
            sleep 1

            attack_root=$(resolve_output_dir "./results/ablation-snapshot-attack/attack/${mode}-${committee_size}-${timestamp}")
            mkdir -p "${attack_root}"

            echo "Running snapshot ablation (attack): repeat=${run_id}/${REPEAT} mode=${mode} committee_size=${committee_size} stall=[${STALL_START},${STALL_END})"
            bash "${SCRIPT_DIR}/attackrun.sh" \
                "${TOTAL_DURATION}" \
                "${committee_size}" \
                "${STALL_START}" \
                "${STALL_END}" \
                "${LOAD}" \
                "${mode}" \
                "${WORKLOAD}" \
                "${attack_root}"
            sleep 1
        done
    done
done

echo "Snapshot attack ablation complete"
