#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
toolchain="nightly-2026-11-01"
nightly_cargo="$(rustup which --toolchain "$toolchain" cargo)"
nightly_bin="${nightly_cargo%/*}"

cd "$repo_root"
export PATH="$nightly_bin:$PATH"
exec cargo fuzz "$@"
