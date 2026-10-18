# Changelog

## [Unreleased]

## [0.1.0] - 2026-10-18

First version.

### Added

- Speaker discovery over mDNS with a dialog to choose which found speakers to add, and manual
  entry by IPv6 address, with the interface index for link-local addresses.
- Added speakers are remembered and reconnected at every start; a scan updates the stored
  address of a remembered speaker that moved.
- Removing a speaker from its device panel after a confirmation.
- One channel strip per speaker with status LEDs, delay (in ms) and standby knobs, an output
  level fader that shows its value while dragging, has a fine mode and takes typed values, input
  meters (KH 750: also output meters) with clip lamps, response time, mute and identify.
- A device panel per speaker with every setting of its model and a list of all values the
  speaker reports.
- A settings dialog for theme, meter range and response times.
- Groups with a group strip (relative fader, mute, linked settings), a master strip, and strips
  reordered by dragging.
- Support for the KH 120 II and the KH 750 with writes; other models read-only.
- Portable settings and log next to the executable.
- A speaker simulator for both models used by the automated tests.
- `check.ps1` quality gates and `build-release.ps1` for the release folder.
