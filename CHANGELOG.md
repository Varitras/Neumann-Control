# Changelog

## [Unreleased]

## [0.1.0] - 2026-10-18

First version.

### Added

- Speaker discovery over mDNS and manual entry by IPv6 address, with the interface index for
  link-local addresses.
- Speaker cards with an output level fader, input meters with clip lamps, mute and identify.
- A per-model settings area and a list of all values the speaker reports.
- Support for the KH 120 II and the KH 750 with writes; other models read-only.
- Portable settings and log next to the executable.
- A speaker simulator for both models used by the automated tests.
- `check.ps1` quality gates and `build-release.ps1` for the release folder.
