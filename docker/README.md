# PRDB client image

The root `Dockerfile` builds a Linux x86_64 image containing `prdb-client`,
TinyCC 0.9.27, the SQL SDK headers and shared library, GCC's linker, and
`patchelf`.
The image connects to a separately running PRDB gateway; it does not start or
initialize a database server. Its default command is `prdb-client --help`.

Build from the repository root:

```sh
docker build -t prdb-client .
podman build -t localhost/prdb-client .
```

Pass any `prdb-client` command after the image name. Mount credentials and
client input files read-only, and mount an output directory when a command
creates a receipt, scope file, or token credential:

```sh
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -v "$PWD/client:/work:rw" \
  prdb-client discover \
    --address 127.0.0.1:8765 \
    --credentials /work/user.credentials
```

When the gateway runs on the Docker host, use `host.docker.internal` on Docker
Desktop. On Linux, add `--add-host=host.docker.internal:host-gateway` and use
that hostname, or connect to the host's reachable address. The gateway must
bind an address reachable from the container; its loopback-only default is
not reachable from a separate container.

For `run-sql` and interactive SQL, the image includes a Debian Bookworm SDK
release at `/opt/prdb/sdk/releases/linux-x86_64/x86_64-v1/debian-bookworm-glibc-2.36/`
and headers at `/opt/prdb/include`. The client command must use a CPU baseline
and runtime profile accepted by the server. These image build arguments select
the SDK release metadata and directory:

```sh
docker build -t prdb-client \
  --build-arg PRDB_CPU_BASELINE=x86_64-v1 \
  --build-arg PRDB_RUNTIME_PROFILE=debian-bookworm-glibc-2.36 \
  --build-arg PRDB_SDK_RELEASE=1 \
  --build-arg PRDB_SDK_ABIS=1,2,3,4,5 .
```

Example SQL invocation (scope and receipt are files in the mounted directory):

```sh
docker run --rm --user "$(id -u):$(id -g)" \
  -v "$PWD/client:/work:rw" prdb-client run-sql \
  --address host.docker.internal:8765 \
  --credentials /work/user.credentials \
  --sql-file /work/query.sql --schema public \
  --scope /work/scope.json --receipt /work/receipt.json \
  --sdk-library /opt/prdb/sdk/releases/linux-x86_64/x86_64-v1/debian-bookworm-glibc-2.36/sdk-1-abis1,2,3,4,5/libprdb_sql.so.1 \
  --include-dir /opt/prdb/include \
  --sdk-release 1 --cpu-baseline x86_64-v1 \
  --runtime-profile debian-bookworm-glibc-2.36 \
  --compiler-identity 'tcc-container-debian-bookworm'
```

The image runs as an unprivileged `prdb` user by default. Supplying the host
UID/GID is useful when bind-mounted output files need to be owned by the host
user. Preserve mode `0600` on credential files; do not bake credentials into
the image or pass token secrets as command-line arguments.

The default build target is the client image. A server container needs a
validated runtime profile and delegated writable cgroup v2 subtree for the
query executor; see [the system service deployment guide](../contrib/systemd/README.md).

## Server image

Build the server target from the same Dockerfile:

```sh
docker build --target server -t prdb-server .
# or
podman build --target server -t localhost/prdb-server .
```

The server image includes the client, supervisor, gateway, executor, optimizer,
bootstrap and maintenance tools, and the matching SQL SDK. Its default command
initializes `/var/lib/prdb/instance` on first start, saves the initial
administrator credential at `/var/lib/prdb/admin.credentials`, and starts the
supervisor plus gateway. Persist all of `/var/lib/prdb`; keep the credential
file private. The default gateway ports are 8765 (query) and 8766 (control).

The server deliberately refuses to start unless `PRDB_CGROUP_PARENT` names an
absolute writable cgroup v2 directory with `cpu`, `memory`, and `pids` both
available and enabled for child cgroups. The container runtime must expose a
delegated writable subtree; mounting `/sys/fs/cgroup` read-write by itself
does not create that delegation. Use a systemd-managed delegated service or a
runtime configuration that delegates those controllers. The image runs as UID
10001, so the persistent data directory must be writable by that UID.

Example after the host has provided a delegated cgroup parent visible at the
same path inside the container:

```sh
install -d -m 0700 "$PWD/prdb-data"
podman run -d --name prdb \
  --cgroupns=host \
  --userns=keep-id:uid=10001,gid=10001 \
  --volume "$PWD/prdb-data:/var/lib/prdb:rw" \
  --volume /sys/fs/cgroup:/sys/fs/cgroup:rw \
  --env PRDB_CGROUP_PARENT=/sys/fs/cgroup/DELEGATED/PARENT \
  --publish 8765:8765 --publish 8766:8766 \
  localhost/prdb-server
```

Replace `DELEGATED/PARENT` with the actual host delegated subtree path. Do not
expose these plaintext TCP ports on an untrusted network. `podman logs prdb`
confirms when initial storage setup completes; the credential secret itself is
not printed. To see the administrator credential, read the mode-0600 file from
the persistent host directory.

Set `PRDB_BIND`, `PRDB_CONTROL_BIND`, `PRDB_RUNTIME_PROFILE`,
`PRDB_CPU_BASELINE`, `PRDB_ADMINISTRATOR`, and `PRDB_CREDENTIAL_FILE` to change
the defaults. The runtime profile must match the SDK release profile built
into the image and accepted by clients. The image does not weaken worker
confinement when cgroup delegation or other required sandbox features are
unavailable.
