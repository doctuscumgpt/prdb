# PRDB client image

The root `Dockerfile` builds a Linux x86_64 image containing `prdb-client`,
TinyCC 0.9.27, the SQL SDK headers and shared library, GCC's linker, and
`patchelf`.
The image connects to a separately running PRDB gateway; it does not start or
initialize a database server. Its default command is `prdb-client --help`.

Build from the repository root:

```sh
docker build -t prdb-client .
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

This is a client image rather than a server deployment image. A server
container needs a validated runtime profile and delegated writable cgroup v2
subtree for the query executor; see [the system service deployment guide](../contrib/systemd/README.md).
