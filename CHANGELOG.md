# Changelog

## [Unreleased]

## [0.1.0] - 2026-10-18

First version.

### Added

- Speaker discovery over mDNS with a dialog to choose which found speakers to add, and manual
  entry by IPv6 address, with the interface index for link-local addresses.
- Added speakers are remembered and reconnected at every start; a scan updates the stored
  address of a remembered speaker that moved.
- Removing a speaker from its card after a confirmation.
- Speaker cards with an output level fader, input meters (KH 750: also output meters) with clip lamps, mute and identify.
- A per-model settings area and a list of all values the speaker reports.
- Support for the KH 120 II and the KH 750 with writes; other models read-only.
- Portable settings and log next to the executable.
- A speaker simulator for both models used by the automated tests.
- `check.ps1` quality gates and `build-release.ps1` for the release folder.
