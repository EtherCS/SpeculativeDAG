#!/bin/bash

set -euo pipefail

ROOT_DIR=${1:-"./results/microbench-$(date +%Y%m%d-%H%M%S)"}
MODE=${MODE:-full}
SWEEP_LOADS=${SWEEP_LOADS:-"25 50 75 100 125"}
COMMITTEE_SIZE=${COMMITTEE_SIZE:-4}
DURATION=${DURATION:-45}

mkdir -p "${ROOT_DIR}"

for load in ${SWEEP_LOADS}; do
    out_dir="${ROOT_DIR}/load-${load}"
    bash scripts/jitterrun.sh \
        "${TOTAL_DURATION:-70}" \
        "${COMMITTEE_SIZE}" \
        "${FAULT_NUM:-1}" \
        "${DELAY_CONNECTION_NUM:-4}" \
        "${JITTER_MS:-2500}" \
        "${JITTER_START_TIME:-10}" \
        "${JITTER_DURATION:-30}" \
        "${load}" \
        "${MODE}" \
        "${out_dir}"
done

echo "Microbenchmark sweep complete under ${ROOT_DIR}"
