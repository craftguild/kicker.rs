#!/bin/sh
# Select an already installed Python 3.11+ for source-only validation; never install one.
set -eu
export PYTHONDONTWRITEBYTECODE=1
for interpreter in python3 python3.15 python3.14 python3.13 python3.12 python3.11; do
  if command -v "$interpreter" >/dev/null 2>&1 && \
    "$interpreter" -c 'import sys; sys.exit(sys.version_info < (3, 11))' >/dev/null 2>&1; then
    exec "$interpreter" "$@"
  fi
done
echo 'Python 3.11 or later is required; no interpreter is installed automatically' >&2
exit 1
