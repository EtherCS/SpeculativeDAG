#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALLER_PWD="$(pwd)"

DURATION=${1:-60}
NODES=${2:-4}
WORKERS=${3:-1}
RATE=${4:-400}
ASYNC_START_SECS=${5:-10}
ASYNC_DURATION_SECS=${6:-50}
EXECUTION=${7:-ordered}
WORKLOAD=${8:-erc20}
EXECUTOR=${9:-sequential}
OUTPUT_DIR=${10:-"./results/jitter-${EXECUTION}-${WORKLOAD}-${EXECUTOR}-$(date +%Y%m%d-%H%M%S)"}

if [[ "${OUTPUT_DIR}" != /* ]]; then
    OUTPUT_DIR="${CALLER_PWD}/${OUTPUT_DIR}"
fi

ASYNC_START_MS=$((ASYNC_START_SECS * 1000))
ASYNC_DURATION_MS=$((ASYNC_DURATION_SECS * 1000))

mkdir -p "${OUTPUT_DIR}"

cd "${SCRIPT_DIR}"

echo "Starting Autobahn jitter evaluation..."
echo "  duration=${DURATION}s nodes=${NODES} workers=${WORKERS} rate=${RATE}"
echo "  asynchrony_start=${ASYNC_START_SECS}s asynchrony_duration=${ASYNC_DURATION_SECS}s"
echo "  execution=${EXECUTION} workload=${WORKLOAD} executor=${EXECUTOR}"

fab jitter \
    --execution="${EXECUTION}" \
    --workload="${WORKLOAD}" \
    --executor="${EXECUTOR}" \
    --nodes="${NODES}" \
    --workers="${WORKERS}" \
    --rate="${RATE}" \
    --duration="${DURATION}" \
    --asynchrony-start="${ASYNC_START_MS}" \
    --asynchrony-duration="${ASYNC_DURATION_MS}" \
    2>&1 | tee "${OUTPUT_DIR}/summary.txt"

LATEST_LOG_DIR="$(find "${SCRIPT_DIR}/logs" -maxdepth 1 -type d -name 'jitter-*' | sort | tail -n 1)"

cat > "${OUTPUT_DIR}/run-meta.txt" <<EOF
experiment=jitter
duration=${DURATION}
nodes=${NODES}
workers=${WORKERS}
rate=${RATE}
asynchrony_start_secs=${ASYNC_START_SECS}
asynchrony_duration_secs=${ASYNC_DURATION_SECS}
execution=${EXECUTION}
workload=${WORKLOAD}
executor=${EXECUTOR}
logs_dir=${LATEST_LOG_DIR}
EOF

if [[ -n "${LATEST_LOG_DIR}" ]]; then
    ln -sfn "${LATEST_LOG_DIR}" "${OUTPUT_DIR}/logs"
fi

echo "Results written to ${OUTPUT_DIR}"
