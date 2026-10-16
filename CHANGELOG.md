# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `contracts/stream/src/roles.rs`: role-alias layer (issue #623) exposing
  `get_fee_admin`/`set_fee_admin`/`revoke_fee_admin` and
  `get_emergency_admin`/`set_emergency_admin`/`revoke_emergency_admin` as thin
  wrappers around the `*_manager`/`*_pause_role` functions. Both naming schemes
  coexist intentionally: `*_manager`/`*_pause_role` are the internal canonical
  names matching the `AdminRole` enum and storage keys, while `*_admin` aliases
  preserve external API compatibility for existing callers and SDK bindings.
  See the "Role Aliases (Issue #623)" section in roles.rs.

### Changed

- Placeholder for upcoming changes.

---

## [0.1.0] - 2026-06-27

### Added

- Initial release of SoroStream smart contracts.
- Initial smart contract implementation.
