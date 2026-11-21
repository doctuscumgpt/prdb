#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Install and start a named PRDB instance for the current systemd user.

Usage:
  install-user-instance.sh --name NAME --port PORT --administrator NAME \
    --runtime-profile NAME --cpu-baseline NAME --runtime-read-execute PATH \
    [--runtime-read-execute PATH ...] [--allowed-dependency NAME ...]

Options:
  --control-port PORT             Control listener port (default: PORT + 1)
  --expires-in-seconds N          Bootstrap credential lifetime (default: 2592000)
  --runtime-read-execute PATH     Validated runtime file, repeatable
  --allowed-dependency NAME       Runtime dependency, repeatable (default: libc.so.6)
  --help

Build artifacts must exist in target/release. Runtime files and profile values
must match a profile validated for this host. The gateway binds to loopback.
EOF
}

die() { printf 'install-user-instance: %s\n' "$*" >&2; exit 2; }
valid_path() {
    [[ "$1" == /* && "$1" != *[[:space:]]* && "$1" != *\\* ]] || die "paths must be absolute, contain no whitespace, and contain no backslashes: $1"
}
normalize_port() {
    local value=$1
    [[ "$value" =~ ^[0-9]{1,5}$ ]] || die "port must be a number in 1..65535: $value"
    value=$((10#$value))
    ((value >= 1 && value <= 65535)) || die "port must be a number in 1..65535: $1"
    printf '%d' "$value"
}

name=
port=
control_port=
administrator=
runtime_profile=
cpu_baseline=
expires=2592000
declare -a runtime_files=()
declare -a dependencies=()
while (($#)); do
    case "$1" in
        --name|--port|--control-port|--administrator|--runtime-profile|--cpu-baseline|--expires-in-seconds|--runtime-read-execute|--allowed-dependency)
            (($# >= 2)) || die "missing value for $1"
            flag=$1 value=$2; shift 2
            case "$flag" in
                --name) name=$value;;
                --port) port=$value;;
                --control-port) control_port=$value;;
                --administrator) administrator=$value;;
                --runtime-profile) runtime_profile=$value;;
                --cpu-baseline) cpu_baseline=$value;;
                --expires-in-seconds) expires=$value;;
                --runtime-read-execute) runtime_files+=("$value");;
                --allowed-dependency) dependencies+=("$value");;
            esac
            ;;
        --help|-h) usage; exit 0;;
        *) die "unknown option: $1";;
    esac
done

[[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,47}$ ]] || die '--name must be 1-48 letters, digits, dot, underscore, or hyphen'
[[ -n "$port" ]] || die '--port is required'
port=$(normalize_port "$port")
((port <= 65534)) || die '--port must be in 1..65534 so a default control port is available'
if [[ -z "$control_port" ]]; then control_port=$((port + 1)); else control_port=$(normalize_port "$control_port"); fi
((port != control_port)) || die 'gateway and control ports must differ'
[[ -n "$administrator" && "$administrator" != *[[:space:]]* && -n "$runtime_profile" && "$runtime_profile" != *[[:space:]]* && -n "$cpu_baseline" && "$cpu_baseline" != *[[:space:]]* ]] || die 'administrator and profile names must be non-empty and contain no whitespace'
[[ "$expires" =~ ^[0-9]+$ ]] && ((10#$expires > 0)) || die '--expires-in-seconds must be positive'
((${#runtime_files[@]})) || die 'supply at least one --runtime-read-execute path'
for path in "${runtime_files[@]}"; do valid_path "$path"; done
for dep in "${dependencies[@]}"; do [[ "$dep" != *[[:space:]]* && "$dep" != *\\* ]] || die 'dependency names may not contain whitespace or backslashes'; done
if ((${#dependencies[@]} == 0)); then dependencies=(libc.so.6); fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
bin_dir="$repo_root/target/release"
for bin in prdb-supervisor prdb-gateway prdb-executor prdb-optimizer prdb-init prdb-maintain prdb-landlock-run; do
    [[ -x "$bin_dir/$bin" ]] || die "missing executable $bin_dir/$bin (build the release binaries first)"
done
[[ -x "$repo_root/engine/tools/with_delegated_cgroup.sh" ]] || die 'missing delegated cgroup launcher'

uid=$(id -u)
home=$(getent passwd "$USER" | cut -d: -f6)
[[ -n "$home" && "$home" == /* && "$home" != *[[:space:]]* ]] || die 'could not resolve an absolute, whitespace-free home directory'
state_root="$home/.local/state/prdb/instances/$name"
config_root="$home/.config/prdb/instances"
exec_root="$home/.local/libexec/prdb"
unit_root="$home/.config/systemd/user"
runtime_root="/run/user/$uid/prdb/$name"
instance_dir="$state_root/instance"
scratch_dir="$state_root/scratch"
socket="$runtime_root/supervisor.sock"
supervisor_unit="prdb-$name-supervisor.service"
gateway_unit="prdb-$name-gateway.service"
supervisor_env="$config_root/$name-supervisor.env"
gateway_env="$config_root/$name-gateway.env"

for path in "$state_root" "$supervisor_env" "$gateway_env" "$unit_root/$supervisor_unit" "$unit_root/$gateway_unit"; do
    [[ ! -e "$path" ]] || die "instance already exists or has conflicting files: $path"
done

mkdir -p "$exec_root" "$unit_root" "$config_root" "$state_root" "$scratch_dir" "$instance_dir"
chmod 700 "$state_root" "$scratch_dir" "$instance_dir"
for bin in prdb-supervisor prdb-gateway prdb-executor prdb-optimizer prdb-init prdb-maintain prdb-landlock-run; do
    install -m 0755 "$bin_dir/$bin" "$exec_root/$bin"
done
install -m 0755 "$repo_root/engine/tools/with_delegated_cgroup.sh" "$exec_root/with_delegated_cgroup.sh"

# Collect every directory needed to traverse from the filesystem root to the
# service executables, instance data, socket, and runtime libraries.
declare -A traverse_seen=()
add_ancestors() {
    local current=$1
    while [[ "$current" != / ]]; do
        traverse_seen["$current"]=1
        current=$(dirname -- "$current")
    done
    traverse_seen[/]=1
}
for path in "$exec_root/prdb-supervisor" "$exec_root/prdb-gateway" "$exec_root/prdb-executor" "$exec_root/prdb-optimizer" "$instance_dir" "$scratch_dir" "$runtime_root" "${runtime_files[@]}"; do
    add_ancestors "$path"
done
traverse_paths=()
for path in "${!traverse_seen[@]}"; do traverse_paths+=("$path"); done
IFS=' ' read -r -a traverse_paths <<< "$(printf '%s\n' "${traverse_paths[@]}" | awk '{print length, $0}' | sort -n | cut -d' ' -f2- | tr '\n' ' ')"

supervisor_args=(--instance-dir "$instance_dir" --service-socket "$socket" --gateway-uid "$uid" --cgroup-parent systemd --executor "$exec_root/prdb-executor" --optimizer "$exec_root/prdb-optimizer" --scratch-parent "$scratch_dir" --runtime-profile "$runtime_profile" --cpu-baseline "$cpu_baseline" --retention-seconds 86400 --max-rows 100000 --max-bytes 67108864 --cpu-quota-us 100000 --cpu-period-us 100000 --memory-max-bytes 536870912 --max-processes 32 --sdk-repository-max-bytes 1073741824)
for path in "${runtime_files[@]}"; do supervisor_args+=(--runtime-read-execute "$path"); done
for dep in "${dependencies[@]}"; do supervisor_args+=(--allowed-dependency "$dep"); done
env_escape_words() { local out= item; for item in "$@"; do [[ -z "$out" ]] || out+='\ '; out+=$item; done; printf '%s' "$out"; }

{
    printf 'PRDB_SUPERVISOR_ARGS=%s\n' "$(env_escape_words "${supervisor_args[@]}")"
    printf 'PRDB_AUDIT_JSONL_PATH=%s\n' "$state_root/audit.jsonl"
    printf 'PRDB_LANDLOCK_TRAVERSE_PATHS=%s\n' "$(env_escape_words "${traverse_paths[@]}")"
    landlock_paths=("$exec_root/prdb-executor" "$exec_root/prdb-optimizer" "${runtime_files[@]}")
    printf 'PRDB_SUPERVISOR_LANDLOCK_PATHS=%s\n' "$(env_escape_words "${landlock_paths[@]}")"
} > "$supervisor_env"
{
    printf 'PRDB_GATEWAY_ARGS=%s\n' "$(env_escape_words --bind 127.0.0.1:""$port"" --control-bind 127.0.0.1:""$control_port"" --service-socket "$socket" --service-uid "$uid")"
    printf 'PRDB_GATEWAY_LANDLOCK_TRAVERSE_PATHS=%s\n' "$(env_escape_words "${traverse_paths[@]}")"
    printf 'PRDB_GATEWAY_LANDLOCK_PATHS=%s\n' "$(env_escape_words "$exec_root/prdb-gateway" "${runtime_files[@]}")"
} > "$gateway_env"
chmod 600 "$supervisor_env" "$gateway_env"

cat > "$unit_root/$supervisor_unit" <<EOF
[Unit]
Description=PRDB supervisor instance $name
After=default.target

[Service]
Type=simple
Delegate=cpu memory pids
RuntimeDirectory=prdb/$name
RuntimeDirectoryMode=0700
EnvironmentFile=$supervisor_env
ExecStart=$exec_root/with_delegated_cgroup.sh $exec_root/prdb-landlock-run --traverse-env PRDB_LANDLOCK_TRAVERSE_PATHS --traverse /dev --read-execute $exec_root/prdb-supervisor --read-write-file /dev/null --writable $state_root --writable $runtime_root --writable-env PRDB_SYSTEMD_CGROUP_PARENT --read-execute-env PRDB_SUPERVISOR_LANDLOCK_PATHS -- $exec_root/prdb-supervisor \$PRDB_SUPERVISOR_ARGS
Restart=on-failure
RestartSec=2s
UMask=0077
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=$state_root $runtime_root

[Install]
WantedBy=default.target
EOF
cat > "$unit_root/$gateway_unit" <<EOF
[Unit]
Description=PRDB gateway instance $name
Requires=$supervisor_unit
After=$supervisor_unit

[Service]
Type=simple
EnvironmentFile=$gateway_env
ExecStart=$exec_root/prdb-landlock-run --traverse-env PRDB_GATEWAY_LANDLOCK_TRAVERSE_PATHS --read-execute $exec_root/prdb-gateway --read-execute-env PRDB_GATEWAY_LANDLOCK_PATHS --read $runtime_root -- $exec_root/prdb-gateway \$PRDB_GATEWAY_ARGS
Restart=on-failure
RestartSec=2s
UMask=0077
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only

[Install]
WantedBy=default.target
EOF

credential="$state_root/admin.credentials"
"$exec_root/prdb-init" --instance-dir "$instance_dir" --administrator "$administrator" --credential-file "$credential" --expires-in-seconds "$expires"
systemctl --user daemon-reload
systemctl --user enable --now "$gateway_unit"
printf 'Installed instance %s on 127.0.0.1:%s (control: 127.0.0.1:%s).\n' "$name" "$port" "$control_port"
printf 'Bootstrap credentials: %s (mode 0600).\n' "$credential"
printf 'Services: %s %s\n' "$supervisor_unit" "$gateway_unit"
