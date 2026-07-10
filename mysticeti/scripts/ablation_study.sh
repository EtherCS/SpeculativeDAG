#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"

EXPERIMENT_KIND=${1:-jitter}
ROOT_DIR=${2:-"./results/ablation-$(date +%Y%m%d-%H%M%S)"}
WORKLOAD=${3:-erc20}
MODES=${MODES:-"full eac no-aps all-skip no-snapshots eager-snapshots"}

if [[ "${ROOT_DIR}" != /* ]]; then
    ROOT_DIR="${CALLER_PWD}/${ROOT_DIR}"
fi

mkdir -p "${ROOT_DIR}"

for mode in ${MODES}; do
    out_dir="${ROOT_DIR}/${WORKLOAD}/${mode}"
    if [ "${EXPERIMENT_KIND}" = "dryrun" ]; then
        bash "${SCRIPT_DIR}/speculative.sh" \
            "${COMMITTEE_SIZE:-4}" \
            "${DURATION:-60}" \
            "${LOAD:-100}" \
            "${mode}" \
            "${WORKLOAD}" \
            "${out_dir}"
    else
        bash "${SCRIPT_DIR}/jitterrun.sh" \
            "${TOTAL_DURATION:-90}" \
            "${COMMITTEE_SIZE:-7}" \
            "${FAULT_NUM:-4}" \
            "${DELAY_CONNECTION_NUM:-4}" \
            "${JITTER_MS:-2500}" \
            "${JITTER_START_TIME:-10}" \
            "${JITTER_DURATION:-90}" \
            "${LOAD:-100}" \
            "${mode}" \
            "${WORKLOAD}" \
            "${out_dir}"
    fi
done

echo "Ablation sweep complete under ${ROOT_DIR} for workload=${WORKLOAD}"
