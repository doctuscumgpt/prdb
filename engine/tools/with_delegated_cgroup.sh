#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
    echo "usage: with_delegated_cgroup.sh COMMAND [ARG ...]" >&2
    exit 2
fi

entry=$(awk -F: '$1 == "0" { print $3 }' /proc/self/cgroup)
case "$entry" in
    /*) ;;
    *) echo "with_delegated_cgroup.sh: process is not in a cgroup v2 hierarchy" >&2; exit 1 ;;
esac

parent="/sys/fs/cgroup$entry"
if [ ! -d "$parent" ] || [ ! -r "$parent/cgroup.controllers" ] || [ ! -w "$parent/cgroup.procs" ]; then
    echo "with_delegated_cgroup.sh: current service cgroup is not writable: $parent" >&2
    exit 1
fi

available=$(cat "$parent/cgroup.controllers")
for controller in cpu memory pids; do
    case " $available " in
        *" $controller "*) ;;
        *) echo "with_delegated_cgroup.sh: service lacks delegated $controller controller" >&2; exit 1 ;;
    esac
done

launcher="$parent/prdb-launcher-$$"
mkdir "$launcher"
printf '%s\n' "$$" > "$launcher/cgroup.procs"
printf '+cpu +memory +pids\n' > "$parent/cgroup.subtree_control"

enabled=$(cat "$parent/cgroup.subtree_control")
for controller in cpu memory pids; do
    case " $enabled " in
        *" $controller "*) ;;
        *) echo "with_delegated_cgroup.sh: failed to enable delegated $controller controller" >&2; exit 1 ;;
    esac
done

export PRDB_SYSTEMD_CGROUP_PARENT="$parent"
export PRDB_TEST_CGROUP_PARENT="$parent"
exec "$@"
