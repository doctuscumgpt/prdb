# Protocol and catalog codec fuzzing

The normal engine remains on the repository's pinned stable Rust toolchain. `cargo-fuzz` uses nightly-only sanitizer and coverage flags, so this isolated harness pins `nightly-2026-11-01` in `rust-toolchain.toml`; it does not change the engine's production or acceptance compiler.

Install the isolated fuzz compiler once from the locked Nix shell:

```sh
nix develop --no-update-lock-file -c rustup toolchain install nightly-2026-11-01 --profile minimal
```

Build or run targets from the repository root through the helper, which puts the pinned nightly Cargo and rustc ahead of the stable shell binaries:

```sh
nix develop --no-update-lock-file -c engine/tools/fuzz_client_protocol.sh build client_decoders
nix develop --no-update-lock-file -c engine/tools/fuzz_client_protocol.sh build client_sequence_state
nix develop --no-update-lock-file -c engine/tools/fuzz_client_protocol.sh build catalog_ddl_codecs
nix develop --no-update-lock-file -c engine/tools/fuzz_client_protocol.sh run client_decoders -- -max_total_time=300 -max_len=1048608 -rss_limit_mb=2048 -dict=fuzz/client_protocol.dict
nix develop --no-update-lock-file -c engine/tools/fuzz_client_protocol.sh run client_sequence_state -- -max_total_time=300 -max_len=32768 -rss_limit_mb=512
nix develop --no-update-lock-file -c engine/tools/fuzz_client_protocol.sh run catalog_ddl_codecs -- -max_total_time=300 -max_len=1048576 -rss_limit_mb=2048
```

`client_decoders` exercises the JSON control decoders, chunk decoder, and incremental PDBN frame decoder under varied input chunking. `client_sequence_state` interprets bounded 19-byte events as request-ID acceptance, stream creation, or streamed response frames and mutates multi-step sequences. `catalog_ddl_codecs` fuzzes registered CAT1 families, policy targets, stored EIR expressions, bounded optimizer request decoders for SELECT/grouping/DELETE/UPDATE, and corresponding plan decoders. Its corpus includes independent valid database and default-expression records plus malformed framing, so mutations reach typed field validation and expression decoding. All inputs are capped at the one-MiB catalog envelope bound. Seed files are in `corpus/`; `client_protocol.dict` provides protocol vocabulary. Fuzzer discoveries are written into the selected corpus directory. Review new inputs and minimize a retained corpus with the pinned helper before committing; crashes go under `artifacts/` and must become ordinary regression tests.

The harness bounds input length and the number of state events. A clean campaign is evidence only for the targets, seed corpus, and elapsed time used; continue campaigns in CI and preserve minimized crash reproducers as regression tests.
