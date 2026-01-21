#!/bin/bash

# Configuration
DURATION=${1:-30}  # Run duration in seconds
COMMITTEE_SIZE=${2:-4}  # Number of validators
FAULT_NUM=${3:-1}   # Number of jitter nodes
DELAY_CONNECTION_NUM=${4:-2} # Number of connections to delay per jitter node
JITTER_MS=${5:-100} # Jitter delay in milliseconds
DURATION_SECS=${6:-15} # Jitter duration in seconds

cargo build

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::speculative_executor=DEBUG,mysticeti_core::block_handler=INFO,

tmux kill-server || true

echo "Starting validators..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cargo run --bin mysticeti -- jitter-run --authority ${i} --committee-size ${COMMITTEE_SIZE} --fault-num ${FAULT_NUM} --delay-connection-num ${DELAY_CONNECTION_NUM} --jitter-ms ${JITTER_MS} --duration-secs ${DURATION_SECS} > v${i}.log.ansi"
done

sleep ${DURATION}

# report the metrics
curl http://0.0.0.0:1504/metrics > ./log0.txt
curl http://0.0.0.0:1505/metrics > ./log1.txt

echo "Stopping validators..."
tmux kill-server