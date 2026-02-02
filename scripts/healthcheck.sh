#!/bin/bash
# Claudima Health Checker - monitors and restarts all bot instances
# Usage: ./scripts/healthcheck.sh [--daemon]

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BINARY="$PROJECT_DIR/target/release/claudima"
DATA_DIR="$PROJECT_DIR/data"
PIDFILE="/tmp/claudima-healthcheck.pid"
LOGFILE="/tmp/claudima-healthcheck.log"
CHECK_INTERVAL=30

# Instance configurations: name:config_path
INSTANCES=(
    "claudima:$DATA_DIR/claudima/claudima.json"
    "oracle:$DATA_DIR/oracle/oracle.json"
    "scout:$DATA_DIR/scout/scout.json"
)

log() {
    echo "$(date '+%Y-%m-%d %H:%M:%S') $1" | tee -a "$LOGFILE"
}

is_running() {
    local config="$1"
    # Extract just the filename pattern (e.g., "data/claudima/claudima.json")
    local config_pattern=$(basename "$(dirname "$config")")/$(basename "$config")
    # Search by config pattern (process may use relative or absolute paths)
    pgrep -f "claudima.*$config_pattern" > /dev/null 2>&1
}

start_instance() {
    local name="$1"
    local config="$2"
    log "[START] Starting $name..."
    cd "$PROJECT_DIR"
    nohup "$BINARY" "$config" --message "Restarted by healthcheck" >> "$LOGFILE" 2>&1 &
    sleep 2
    if is_running "$config"; then
        local config_pattern=$(basename "$(dirname "$config")")/$(basename "$config")
        log "[OK] $name started successfully (PID: $(pgrep -f "claudima.*$config_pattern"))"
        return 0
    else
        log "[ERROR] Failed to start $name"
        return 1
    fi
}

check_all() {
    local all_ok=true
    for instance in "${INSTANCES[@]}"; do
        local name="${instance%%:*}"
        local config="${instance#*:}"

        if [ ! -f "$config" ]; then
            log "[SKIP] $name - config not found: $config"
            continue
        fi

        if is_running "$config"; then
            log "[OK] $name is running"
        else
            log "[DOWN] $name is NOT running"
            start_instance "$name" "$config" || all_ok=false
        fi
    done
    $all_ok
}

status() {
    echo "Claudima Health Status"
    echo "======================"
    for instance in "${INSTANCES[@]}"; do
        local name="${instance%%:*}"
        local config="${instance#*:}"

        if [ ! -f "$config" ]; then
            echo "  $name: [SKIP] config not found"
            continue
        fi

        if is_running "$config"; then
            local config_pattern=$(basename "$(dirname "$config")")/$(basename "$config")
            local pid=$(pgrep -f "claudima.*$config_pattern")
            echo "  $name: [RUNNING] PID $pid"
        else
            echo "  $name: [STOPPED]"
        fi
    done
}

daemon_loop() {
    if [ -f "$PIDFILE" ]; then
        local old_pid=$(cat "$PIDFILE")
        if kill -0 "$old_pid" 2>/dev/null; then
            echo "Healthcheck daemon already running (PID $old_pid)"
            exit 1
        fi
    fi

    echo $$ > "$PIDFILE"
    trap "rm -f $PIDFILE" EXIT

    log "[DAEMON] Healthcheck daemon started (PID $$)"
    log "[DAEMON] Checking every ${CHECK_INTERVAL}s"

    while true; do
        check_all
        sleep "$CHECK_INTERVAL"
    done
}

stop_daemon() {
    if [ -f "$PIDFILE" ]; then
        local pid=$(cat "$PIDFILE")
        if kill -0 "$pid" 2>/dev/null; then
            log "[DAEMON] Stopping daemon (PID $pid)"
            kill "$pid"
            rm -f "$PIDFILE"
            echo "Daemon stopped"
        else
            rm -f "$PIDFILE"
            echo "Daemon not running (stale pidfile removed)"
        fi
    else
        echo "Daemon not running"
    fi
}

case "${1:-check}" in
    check)
        check_all
        ;;
    status)
        status
        ;;
    daemon|--daemon)
        daemon_loop
        ;;
    stop)
        stop_daemon
        ;;
    *)
        echo "Usage: $0 {check|status|daemon|stop}"
        echo "  check  - Check and restart dead instances (default)"
        echo "  status - Show status of all instances"
        echo "  daemon - Run as background daemon"
        echo "  stop   - Stop the daemon"
        exit 1
        ;;
esac
