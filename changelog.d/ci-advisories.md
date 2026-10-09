### Added

- Added a CI job `advisories` that runs `cargo deny check advisories` to guard against
  known security advisories in dependencies.
- Accepts two advisories as exceptions:
  - RUSTSEC-2023-0071 (rsa: verify-only) – the rsa crate is only used for verifying
    signatures, not for private-key operations, so the Marvin attack does not apply.
  - RUSTSEC-2026-0215 (smallstr: unmaintained) – the smallstr crate is unmaintained but
    is only used via the yrs CRDT library, and there is no alternative; we monitor the
    situation.