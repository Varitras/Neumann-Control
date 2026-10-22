# Changelog

## [Unreleased]

### Added

- An EQ window per speaker: a computed response graph of every EQ and their sum, band points to
  drag, and a table of the bands. Each change writes one band only and can be undone.
- A history of every setting the app wrote in this session (**History** in the toolbar), with
  **Undo** on each entry and Ctrl+Z for the newest. A group or master move is one entry. When a
  value changed since, the app asks before writing the old one back.
- The held peak of each meter as a number under its channel.
- An **EQ** button on each strip, opening the speaker's EQ window.
- **Make flat** in the EQ window: level and boost of every band of an EQ to 0 dB, after a
  question; not for the KH 750 crossovers.
- **Reset** in the EQ window: every band of an EQ off and back to PARAMETRIC, 100 Hz, Q 0.7,
  level and boost 0 dB, after a question; protected like "Make flat".
- **Undo** in the EQ window and Ctrl+Z there: takes back the speaker's newest EQ change.
- Trends of the temperature and the warnings over the last two hours of the session in the
  device panel.
- **Input mix** per output (input A and B, −100 to 0 dB) and **Mute persist** in the device
  panel; the KH 750 also shows the position of its AUTO STANDBY switch.
- **Compare with** in the EQ window: the EQ sum of other speakers on the same output as a dashed
  line.
- Settings presets (chosen settings under a name) and EQ presets (all bands of an EQ), each loaded
  with a preview as one history entry.
- **Full scan** in the device panel: a report of everything a speaker answers, without its
  identifiers, next to the executable.
- A strip with the Windows volume of a chosen playback device, with mute and peak meters.
- Each EQ band has its own colour and its number in its point; every band's own curve is drawn
  thin, the chosen band's with its area. Colours and which band curves show are set in the
  settings.
- **Import…** and **Export…** in the EQ window: filter lines of Equalizer APO and of Room EQ
  Wizard's "Generic" export, with a preview and one history entry; export as APO lines. Shelving
  filters are converted between the files' Q and the speakers' slope.
- **Protect crossovers** in the settings (on by default); off, "Make flat", "Reset", EQ presets
  and the import work on the KH 750 crossovers too, with a warning.
- Strips you can customize per model: hide, swap and move what a strip shows besides its fader,
  put any setting of the device panel on it, from a context menu or in an edit mode (**↕** in the
  toolbar). Mute and identify are listed in the device panel too.

### Changed

- On the KH 750 the outputs are named after what they drive: Subwoofer (its own woofer), Output A
  and Output B (its two output sockets), formerly Output 1 and Output 2.
- The language follows the Windows display language unless one is chosen in the settings.
- Standby time and standby level can be set on the KH 750 too.
- Wider meters; the held peak is a thin line instead of a lit segment.
- Remembered speakers are looked for without asking: a silent scan at the start and when one stays
  unreachable for 30 seconds (then at most every 5 minutes) follows them, by serial, to a changed
  address or interface index.

### Fixed

- Every write to the KH 120 II's 20-band EQ 3 was refused by the speaker ("code 413"): the
  request was one value too long for it.
- The EQ graph drew bands near the top too wide (a 14 kHz peak by up to 1.3 dB): it now draws
  the speakers' digital filters at 48 kHz.
- The EQ graph drew shelving filters too steep, with an overshoot the speaker does not have: their
  Q is the shelf's slope.
- A value that rounds to zero showed a minus ("-0 dB").
- Number fields with a converted unit showed the device's unit as well ("0 1/48 kHz ms").
- A second click on a switch before the speaker had answered the first repeated it (two quick
  clicks on mute muted twice), and a fader or knob could jump back to its old value for a moment
  after a write.
- **Undo** right after a write asked whether the value had changed since, and wrote nothing.
- The mouse wheel stepped a fader or knob from the level before the last drag, not from the shown
  one.
- The MAC addresses in **All values** showed only `…"]`; each address is now masked on its own.
- A speaker name announced with the serial in other letter case showed the serial unmasked.

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
- A settings dialog for theme, language, meter range and response times.
- German and English user interface, switched at once; German shows a decimal comma.
- A compact window mode (Ctrl+K) with one row per ungrouped speaker and per group and a master
  row, with its own window size and position.
- Groups with a group strip (relative fader, mute, linked settings), a master strip, and strips
  reordered by dragging.
- Support for the KH 120 II and the KH 750 with writes; other models read-only.
- An application icon for the window, the taskbar and the executable.
- Portable settings and log next to the executable; a settings value this version does not
  understand falls back to its default without losing the rest, and the file is copied aside.
- A speaker simulator for both models used by the automated tests.
- `check.ps1` quality gates and `build-release.ps1` for the release folder.
