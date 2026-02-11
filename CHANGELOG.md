# Changelog

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
