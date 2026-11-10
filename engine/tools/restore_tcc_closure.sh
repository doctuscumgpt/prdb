#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
profile_dir=$(dirname -- "$script_dir")
(
	cd "$profile_dir"
	sha256sum -c SHA256SUMS
)

tcc_root=$(cat "$script_dir/TCC_ROOT.txt")
case "$tcc_root" in
	/nix/store/*) ;;
	*) echo "unexpected TinyCC root path: $tcc_root" >&2; exit 1 ;;
esac

NIX_BIN=${NIX_BIN:-nix}
if [ -n "${NIX_STORE_URI:-}" ]; then
	exec "$NIX_BIN" --store "$NIX_STORE_URI" copy --no-check-sigs --from "file://$script_dir/tcc-cache" "$tcc_root"
fi
exec "$NIX_BIN" copy --no-check-sigs --from "file://$script_dir/tcc-cache" "$tcc_root"
