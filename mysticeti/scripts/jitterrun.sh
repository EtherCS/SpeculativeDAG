#!/bin/bash

# Configuration
DURATION=${1:-90}  # Run duration in seconds
COMMITTEE_SIZE=${2:-7}  # Number of validators
FAULT_NUM=${3:-1}   # Number of jitter nodes
DELAY_CONNECTION_NUM=${4:-4} # Number of connections to delay per jitter node
JITTER_MS=${5:-2500} # Jitter delay in milliseconds
DURATION_SECS=${6:-50} # Jitter duration in seconds

cargo build 2>&1 >/dev/null | tail -n 10

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::speculative_executor=DEBUG,mysticeti_core::block_handler=INFO,

tmux kill-server || true

echo "Starting validators..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cargo run --bin mysticeti -- jitter-run --authority ${i} --committee-size ${COMMITTEE_SIZE} --fault-num ${FAULT_NUM} --delay-connection-num ${DELAY_CONNECTION_NUM} --jitter-ms ${JITTER_MS} --duration-secs ${DURATION_SECS} > v${i}.log.ansi"
done

sleep ${DURATION}

# report the metrics
curl http://0.0.0.0:$((1500 + COMMITTEE_SIZE + 1))/metrics > ./log0.txt
curl http://0.0.0.0:$((1500 + COMMITTEE_SIZE + 2))/metrics > ./log1.txt

echo "Stopping validators..."
tmux kill-server