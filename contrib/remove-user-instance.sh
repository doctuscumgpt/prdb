#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Remove a named PRDB instance belonging to the current systemd user.

Usage: remove-user-instance.sh --name NAME [--keep-data] [--yes]

By default this disables/stops the instance and permanently deletes its database,
bootstrap credential, audit log, and per-instance configuration. --keep-data
preserves the database and audit log under ~/.local/state/prdb/instances/NAME.
Shared PRDB executables are retained for other instances.
EOF
}
die() { printf 'remove-user-instance: %s\n' "$*" >&2; exit 2; }

name=
keep_data=0
assume_yes=0
while (($#)); do
    case "$1" in
        --name) (($# >= 2)) || die 'missing value for --name'; name=$2; shift 2;;
        --keep-data) keep_data=1; shift;;
        --yes|-y) assume_yes=1; shift;;
        --help|-h) usage; exit 0;;
        *) die "unknown option: $1";;
    esac
done
[[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,47}$ ]] || die '--name must be 1-48 letters, digits, dot, underscore, or hyphen'
if ((!assume_yes)); then
    if ((keep_data)); then action="stop services and remove configuration for '$name' (keep database data)";
    else action="stop services and permanently delete all data for '$name'"; fi
    printf "About to %s. Continue? [y/N] " "$action" >&2
    read -r answer
    [[ "$answer" == y || "$answer" == Y || "$answer" == yes || "$answer" == YES ]] || { printf 'Cancelled.\n' >&2; exit 1; }
fi

uid=$(id -u)
home=$(getent passwd "$USER" | cut -d: -f6)
[[ -n "$home" && "$home" == /* && "$home" != *[[:space:]]* ]] || die 'could not resolve an absolute, whitespace-free home directory'
state_root="$home/.local/state/prdb/instances/$name"
config_root="$home/.config/prdb/instances"
unit_root="$home/.config/systemd/user"
runtime_root="/run/user/$uid/prdb/$name"
supervisor_unit="prdb-$name-supervisor.service"
gateway_unit="prdb-$name-gateway.service"

systemctl --user disable --now "$gateway_unit" "$supervisor_unit" 2>/dev/null || true
rm -f -- "$unit_root/$gateway_unit" "$unit_root/$supervisor_unit" \
    "$config_root/$name-gateway.env" "$config_root/$name-supervisor.env"
systemctl --user daemon-reload
if [[ -L "$state_root" ]]; then die "refusing to remove symlinked state directory: $state_root"; fi
if ((keep_data)); then
    rm -f -- "$state_root/admin.credentials"
    printf 'Removed services and configuration for %s. Data remains at %s.\n' "$name" "$state_root"
else
    rm -rf -- "$state_root"
    printf 'Removed instance %s and its data.\n' "$name"
fi
rmdir -- "$runtime_root" 2>/dev/null || true
rmdir -- "$(dirname -- "$runtime_root")" 2>/dev/null || true
