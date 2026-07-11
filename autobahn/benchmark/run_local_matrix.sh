#!/usr/bin/env bash

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_FILE="${LOG_FILE:-${SCRIPT_DIR}/matrix-local-$(date +%Y%m%d-%H%M%S).log}"
mkdir -p "$(dirname "${LOG_FILE}")"

cd "${SCRIPT_DIR}"

echo "Local benchmark matrix started at $(date)" > "${LOG_FILE}"
echo "Log file: ${LOG_FILE}" >> "${LOG_FILE}"

for execution in ordered speculative; do
    for workload in erc20 weth uniswap; do
        for rate in 400 4000; do
            for nodes in 4 10; do
                echo "" >> "${LOG_FILE}"
                echo "=== fab local --execution=${execution} --workload=${workload} --rate=${rate} --nodes=${nodes} --runs=2 ===" \
                    >> "${LOG_FILE}"
                echo "Started at $(date)" >> "${LOG_FILE}"

                fab local \
                    --execution="${execution}" \
                    --workload="${workload}" \
                    --rate="${rate}" \
                    --nodes="${nodes}" \
                    --runs=2 \
                    >> "${LOG_FILE}" 2>&1
                status=$?

                echo "Exit status: ${status}" >> "${LOG_FILE}"
                echo "Finished at $(date)" >> "${LOG_FILE}"
            done
        done
    done
done

echo "Local benchmark matrix finished at $(date)" >> "${LOG_FILE}"
echo "Results were written to ${LOG_FILE}"
