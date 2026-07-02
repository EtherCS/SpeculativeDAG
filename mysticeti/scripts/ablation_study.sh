#!/bin/bash

set -euo pipefail

EXPERIMENT_KIND=${1:-jitter}
ROOT_DIR=${2:-"./results/ablation-$(date +%Y%m%d-%H%M%S)"}
MODES=${MODES:-"full eac no-aps no-snapshots eager-snapshots"}

mkdir -p "${ROOT_DIR}"

for mode in ${MODES}; do
    out_dir="${ROOT_DIR}/${mode}"
    if [ "${EXPERIMENT_KIND}" = "dryrun" ]; then
        bash speculative.sh "${COMMITTEE_SIZE:-4}" "${DURATION:-60}" "${mode}" "${out_dir}"
    else
        bash jitterrun.sh \
            "${TOTAL_DURATION:-90}" \
            "${COMMITTEE_SIZE:-7}" \
            "${FAULT_NUM:-4}" \
            "${DELAY_CONNECTION_NUM:-4}" \
            "${JITTER_MS:-2500}" \
            "${JITTER_START_TIME:-10}" \
            "${JITTER_DURATION:-90}" \
            "${LOAD:-100}" \
            "${mode}" \
            "${out_dir}"
    fi
done

echo "Ablation sweep complete under ${ROOT_DIR}"
