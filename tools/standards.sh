#!/bin/sh
# The matching common entry owns execution; this only selects installed tools.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$root"
if [ -f .craftguild/gpg-failure ]; then
    echo 'OPENPGP_FAILURE: pause the goal before further work' >&2
    exit 71
fi
export RUSTUP_AUTO_INSTALL=0 CARGO_NET_OFFLINE=true
case "${1:-}" in
    check|test) : "${PROFILE:=debug}"; export PROFILE ;;
esac
exec sh scripts/python.sh standards/launcher.py "$@"
