#!/usr/bin/env bash
# Fails if the exe or the installer is over its budget (RUST_PORT_PLAN.md §2).
# Usage: size-gate.sh <launchpad.exe> [<installer.exe>]
set -euo pipefail
check() {
  local file=$1 limit_mib=$2 bytes
  bytes=$(stat -c %s "$file")
  local mib=$(awk "BEGIN { printf \"%.2f\", $bytes / 1048576 }")
  if [ "$bytes" -gt $((limit_mib * 1048576)) ]; then
    echo "::error::$file is $mib MiB, over its $limit_mib MiB budget."
    return 1
  fi
  echo "$file: $mib MiB (budget $limit_mib MiB)"
}
check "$1" 13
if [ $# -ge 2 ]; then check "$2" 5; fi
