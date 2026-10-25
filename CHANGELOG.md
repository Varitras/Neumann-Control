# Changelog

## [Unreleased]

### Fixed

- **Check for updates** counts pre-releases: with only a pre-release published it said there was
  no release yet, and it would not have told of a newer pre-release.

## [0.1.17] - 2026-10-25

First public version, an alpha: expect rough edges and changes between versions.

### Added

- **Finding speakers**: a scan over mDNS with a dialog to choose which found speakers to add
  (**Scan again** keeps the list and the checks), or a speaker added by its IPv6 address with the
  interface index for link-local addresses. Added speakers are remembered and reconnected at every
  start; a silent scan at the start and when one stays unreachable for 30 seconds (then at most
  every 5 minutes) follows them, by serial, to a changed address or interface index. A speaker
  that is off keeps the name and model it last reported.
- **The desk**: one channel strip per speaker with LEDs for connection, clip, device warning and
  LOCAL, the input interface, the current input and the standby countdown, an output level fader
  (0.5 dB steps, finer with Shift, mouse wheel, typed values), input meters (on the KH 750 also
  its output meters) with clip lamps and the held peak as a line and a number, the response
  time, and mute, identify and EQ; on the KH 120 II also auto standby, on the KH 750 the mutes of
  outputs A and B and the temperature. What a strip shows can be changed per model or per
  speaker, from a context menu or in an edit mode; strips have colours and are reordered by
  dragging. Without a saved size the window fits the desk or the compact rows until you size it.
- **Device panel** per speaker in a window of its own, with tabs (overview, outputs side by
  side, inputs, standby, device): every setting the catalog knows for the model, a control where
  the model may write it and the value greyed where not, the input mix and mute persist of each
  output, a two-hour trend of the temperature and the warnings (on models with a temperature),
  **All values** with every value the speaker reports, and **Full scan**, a report without
  serial, MAC address, name or network address for collecting what other models report (with an
  issue form for sending it).
- **Presets** of chosen settings and of EQs; loading one compares each change, now and as it
  would be written, and writes it as one history entry.
- **Groups and master**: a group strip moves and mutes its members, links level, mute, auto
  standby or input interface, and copies a linked value to the others; an optional master strip
  moves and mutes every speaker.
- **EQ window** per speaker: a computed response graph of every EQ of an output and their sum
  (the speakers' digital filters at 48 kHz), band points to drag and to turn with the mouse wheel,
  a table of the bands that continues in further columns where the window is wide enough,
  **Make flat**, **Reset**, **Undo**, **Compare with** other speakers, and import of Equalizer APO
  and Room EQ Wizard filter files with a preview, export as APO lines. On the KH 750 the outputs
  are Subwoofer, Output A and Output B, the crossovers lie one per input, and **Protect
  crossovers** keeps the whole-EQ actions away from them.
- **History and undo**: every write of the session (the newest 500), each one revertible, Ctrl+Z
  for the newest; when a value changed since, the app asks before writing the old one back.
- **Compact mode** (Ctrl+K): one row per speaker or group in a small window, with buttons chosen
  per model.
- **Keyboard shortcuts**: Ctrl+↑/↓ moves every speaker by 1 dB, Ctrl+M mutes or unmutes them all,
  Ctrl+D dims them by 20 dB and restores their levels; with a setting also from other programs
  (Ctrl+Shift+Alt).
- **Windows volume** of a playback device as a strip with mute and a peak meter per channel.
- **Settings** in a window of their own: theme, language (following Windows unless chosen),
  meter range, delays in milliseconds or meters, response times, master strip, a layout per
  speaker, global shortcuts, a detailed log for troubleshooting, crossover protection, EQ colours
  and band curves, and the Windows volume strip. Windows of their own open where the last of their
  kind was left (the first time over the middle of the main window), maximized ones maximized.
- **Help**: the manual as an HTML file next to the executable, the wiki with the protocol
  reference, and **About** with a check for updates that asks GitHub only when clicked.
- Writes stay safe: a value is written when a gesture ends, a typed value outside the range is
  refused with the reason, and models whose behaviour is unknown are read-only; the KH 120 II and
  the KH 750 can be written.
- Serial numbers and addresses are masked in the interface and the log; an unreadable or partly
  understood settings file is copied aside and never written over unread.
- Screen reader names for strips, compact rows, knob fields and EQ band types.
- German and English interface, light and dark theme; portable: one executable with its
  settings and log next to it, one app per folder.
