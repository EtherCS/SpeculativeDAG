#!/bin/bash

# Shared process lifecycle helpers for local experiment runners.
VALIDATOR_PIDS=()

register_validator_pid() {
    local pid=$1
    VALIDATOR_PIDS+=("${pid}")
}

stop_validator_pids() {
    local pids=("$@")
    if (( ${#pids[@]} == 0 )); then
        return
    fi

    local pid
    for pid in "${pids[@]}"; do
        kill "${pid}" 2>/dev/null || true
    done

    local attempt
    local alive
    for attempt in $(seq 1 50); do
        alive=0
        for pid in "${pids[@]}"; do
            if kill -0 "${pid}" 2>/dev/null; then
                alive=1
                break
            fi
        done
        if (( alive == 0 )); then
            return
        fi
        sleep 0.1
    done

    for pid in "${pids[@]}"; do
        if kill -0 "${pid}" 2>/dev/null; then
            echo "Force-stopping validator process ${pid}" >&2
            kill -KILL "${pid}" 2>/dev/null || true
        fi
    done
}

stop_registered_validators() {
    stop_validator_pids "${VALIDATOR_PIDS[@]}"
    tmux kill-server 2>/dev/null || true
    # The pane PID can be a shell whose child survives after tmux exits.
    stop_stale_validators
    VALIDATOR_PIDS=()
}

stop_stale_validators() {
    if ! command -v pgrep >/dev/null 2>&1; then
        return
    fi

    local stale_pids=()
    local pid
    while IFS= read -r pid; do
        if [[ -n "${pid}" ]]; then
            stale_pids+=("${pid}")
        fi
    done < <(pgrep -f "${BIN_PATH} (dry-run|jitter-run|attack-run)( |$)" || true)

    if (( ${#stale_pids[@]} > 0 )); then
        echo "Stopping ${#stale_pids[@]} stale validator process(es) from an earlier run..."
        stop_validator_pids "${stale_pids[@]}"
    fi
}
