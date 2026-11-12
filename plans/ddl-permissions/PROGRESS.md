# Permission DDL Progress

## DP-1 / DP-2 / NP-6: column grant authority through separate service processes — verified

- Contract: [permission DDL](../../specs/ddl-permissions.md) and [client protocol](../../specs/client-protocol.md). An authenticated identity cannot grant privileges without current object ownership, inherited `MANAGE_GRANTS`, or database-administrator authority. An owner may grant a specific existing column privilege to a user.
- Regression: the production gateway/supervisor process test creates a table as its regular schema owner. A second authenticated user attempts to grant itself column-level SELECT and receives an administrative rejection. The owner then grants SELECT on that one column through `AdminClient`; dropping the table removes the grant before the recipient is dropped.
- Verification: the focused `supervisor_and_gateway_binaries_commit_a_confined_submission` test passed in the delegated `systemd --user` service. `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, `git diff --check`, and the full serial workspace suite passed; sandbox integration tests were enabled and zero tests were ignored. The gateway fixture uses three control workers for the administrator, owner, and recipient connections that coexist during the check.
- Scope: covers object-grant actor authorization and successful column-grant publication across real service processes. It does not execute a query as the recipient or cover policy requirements and grant-provenance inspection.
