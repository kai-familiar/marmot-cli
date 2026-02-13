# Changelog

## [0.2.0] - 2026-02-13

### Added

- **NIP-46 Remote Signing Support** (Closes #5)
  - `--bunker` flag and `NOSTR_BUNKER` env var for bunker:// URI
  - `init --bunker` command for first-time bunker setup
  - `migrate-to-bunker` command for atomic nsec → bunker migration
  - `signer-status` command to inspect signing mode and connection info
  - Bunker connection config persisted in `marmot.bunker.json` (auto-reconnect)
  - Client keypair generated and stored for stable NIP-46 sessions
  - All signing operations (events, gift-wrap, NIP-44 encryption) routed through bunker
  - Audit logging of all signing requests to `marmot.audit.jsonl`
  - Graceful bunker-offline handling with clear error messages
  - Backward compatible: `--nsec` / `NOSTR_NSEC` still works as before

### Changed

- `whoami` now shows signing mode (direct/bunker)
- `nostr-connect` v0.44.0 added as dependency
- Signer abstraction (`MarmotSigner`) unifies direct and remote signing
- Warning emitted when using direct nsec in long-running processes

## [Unreleased] - 2026-02-11

### Changed

- **BREAKING:** Updated to MDK 0.5.3 / OpenMLS 0.8.0
  - Fixes HIGH severity security advisory GHSA-8x3w-qj7j-gqhf (improper tag validation in openmls)
  - Updated nostr crate from 0.43 to 0.44
  - Migration schema changed (V001... format instead of V100...)
  
- **BREAKING:** Existing databases are incompatible with this version
  - The MDK unified storage architecture requires a fresh database
  - Backup your `~/.marmot-cli/marmot.db` before upgrading
  - You will need to re-publish your key package and be re-invited to groups
  - Run `./marmot-cli/marmot publish-key-package` after upgrading

### API Changes

- `MdkSqliteStorage::new()` now requires keyring integration
- Using `MdkSqliteStorage::new_unencrypted()` for CLI compatibility
- `get_pending_welcomes()` now takes optional pagination parameter
- `Timestamp::as_u64()` deprecated, using `as_secs()` instead

### Security

- OpenMLS 0.8.0 resolves security advisory for improper tag validation
- Time crate updated to 0.3.47 (stack exhaustion DoS fix)
- Bytes crate updated to 1.11.1 (integer overflow fix)

---

*This update tracks upstream MDK changes from marmot-protocol/mdk commits 913e541, 5952e36, bf4b822, 5ef0c60.*
