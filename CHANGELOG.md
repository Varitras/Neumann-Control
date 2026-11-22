# Changelog

## [Unreleased]

### Added

- When a scan finds no speakers, the dialog now says what usually causes it (a firewall blocking
  the speaker search, or a different network), how to add a speaker by address instead, and
  has a button that opens the manual (or says there that the manual is missing).

### Fixed

- The band colours in the settings wrap to the window's width and stand in even columns; a
  narrow window no longer cuts off the last bands.
- The lists behind "Swap for…" and "+ Add" on a strip scroll when the window is lower than
  the list; with a KH 750 the last settings were out of reach.
- A scroll bar's handle now shows at all times, not only under the mouse pointer. To match,
  text inputs are darker in the dark theme and white in the light theme.
- Text and cursor in a text field sit in its middle, not slightly above it.
- A value the speaker confirms differently than it was sent counts at once, also while another
  write to the same setting is on its way: the next change starts from it, and taking a change
  back writes it, not the value sent.
- Loading a preset no longer says the speaker already has its values when nothing could be
  compared: an EQ value went missing, the speaker is gone, or the preset holds only values this
  model does not have.
- The Windows volume strip, when switched on in the settings, now also shows before any speaker
  is added.
- Undo after a write the speaker answered with "not found" no longer sends the value it refused.
- Undoing an EQ change writes nothing to a speaker if one of its old values cannot be written,
  instead of switching a band back on with only part of its values.
- Saving or replacing a settings preset waits for checked values the speaker has not reported yet
  and names them; before, they were left out and a replaced preset lost them.
- A failed EQ export keeps the file it was to replace and leaves no temporary file beside it.
- An EQ import is worked out from the speaker's values when you press **Write**: bands the file
  leaves out keep changes made meanwhile, and if the EQ is no longer reported the preview stays
  open and says so instead of closing without writing.
- Enter in the name field no longer writes back the old name after the speaker was renamed
  elsewhere.
- Dragging an EQ point up or down no longer changes its frequency, and dragging it sideways no
  longer changes its level.
- Removing a speaker removes the right one when two speakers were remembered at the same address.
- If the settings file or its desk part cannot be read, **Protect crossovers** is switched on.
- A speaker that reports no name, firmware, model or serial is now used with that field empty
  instead of being shown as unreachable, and one added from a scan is no longer refused because
  its serial holds unusual characters.
- Full-scan reports and **All values** hide network addresses also when written with a port, a
  prefix, a full stop after them, a label before them (`ip:`) or in angle brackets; copied
  refusal texts are masked too.
- Very long model, firmware and history texts from a speaker are cut, as other device texts are.
- The third-party licence list in a release no longer lists the app's own parts.
- Two libraries the app uses are updated to fix reported safety problems.
- On a screen scaled above 100 %, a window without a size of its own keeps growing with the
  speakers added; it stopped after the first size.
- A size you give the empty window before adding speakers is kept and saved.
- Leaving the app saves where its other windows were, also while the main window still fits
  itself to the speakers.
- Holding **Ctrl+M** or **Ctrl+D** switches mute or dim once, not again with every key repeat.
- An EQ band change the speaker confirms for that band only no longer leaves the other bands
  without values.
- A meter whose speaker sends no reading shows none instead of staying at the last level, and
  a clip lamp no longer stays lit after its speaker became unreachable.
- The meter readout below a falling bar changes four times a second, as intended, not with every
  reading.
- A speaker with several global network addresses is reached at the same one after every scan.
- The Windows volume strip asks Windows for its playback device again after a read failed, and
  reads the channel names again when the device reports another number of channels.
- Compact mode no longer redraws ten to twenty times a second for the Windows volume strip it
  does not show.
- **Scan again** keeps the list and your checks when the scan fails, and keeps a speaker's serial
  when the new scan found it without one.
- **All values** and **Full scan** share one read of the speaker instead of reading it twice
  when started together, and a failed **All values** read is written to the log.
- Loading a preset on a speaker in a group passes linked settings (level, mute and the like) on
  to the other members, as every other change does.
- Group and master controls and linked settings leave out members whose model cannot change the
  setting, instead of showing a refusal on their strip.
- The load preset dialog names skipped settings as everywhere else, not by their internal path.
- An EQ refusal disappears with the next change or when another EQ is shown.
- Reading back an EQ file the app exported changes nothing; every shelf was written again.
- Among overlapping EQ points, the one highlighted is the one a click, drag or wheel changes.
- **Ctrl+Z** in the EQ window waits, as in the main window, while a question waits for an answer.
- If the app's folder cannot be written, the notice says so and the settings no longer offer the
  detailed log.
- An unreadable **Show response times** setting falls back to on, as on a first start.
- Settings are saved once you let go of a colour you drag in its picker, not on every frame.
- A further window of a kind opens on the screen even when the first was left at its edge.
- With two windows of a kind open, the place kept is that of the one you moved last; a window
  only opened there does not move the place where the next one opens.
- On Linux, after starting or switching to a mode whose window was maximized, the app waits up to
  a second for the window to be maximized before it notes the window's size, so another size is
  not kept for it.
- On Windows, a window that opens on a monitor with another scale than the main window is moved
  to the place it was left, in that monitor's scale.
- The log no longer holds a detailed line about the window's place when **Detailed log** is
  off.
- The desk's window can no longer be made so narrow that the toolbar's buttons and the average
  response time run over each other; the compact window can still be narrower.
- Compact mode without speakers says how to get back to the desk, where speakers are added; it
  named buttons compact mode does not have.
- The history names the bands an EQ change switched on or off and the one speaker it went to;
  it said "all bands", "2 speakers" and showed "null" for bands it left alone.
- The Windows volume fader shows its move in percent while dragged, not in dB.
- A knob or fader that cannot be used no longer offers Shift or a double click in its hint.

## [0.2.1] - 2026-11-03

### Changed

- **Import…** and **Export…** take no band pass (`BP`) any more: the speakers' band pass peaks at
  its Q, Equalizer APO's at 0 dB, so it was off by that peak (12 dB at Q 4). An exported band pass
  is written as a comment and listed.

### Fixed

- An EQ preset or import switches a band on only after its other values are written, and off
  before, and taking it back does the same; a band could sound its old values first, and keep
  them after an outage.
- An Equalizer APO file with several `Device:` or `Stage:` sections is refused, as one with several
  channels is; their filters were stacked into one EQ.
- Every control starts from the value just sent: a second choice in a list, a typed number or
  text, a knob or fader turned again and a change in the EQ window before the speaker answered
  the first no longer repeat or drop it.
- A setting the speaker answered as not found is no longer kept in the history as written, and
  taking a change back that way no longer counts as done.
- Loading or saving a preset right after a change, and a second click on a group's mute or a
  move of its fader before the speakers answered the first, start from the new value instead
  of the one before.
- A click right after a speaker connects goes out at once instead of after all its values were
  read (about two seconds on a KH 750).
- A setting changed on the speaker while a file dialog was open for long is read again as soon
  as the dialog closes, not up to half a minute later.
- The preset save dialog shows the value just sent, which is what it saves.
- **Ctrl+Z** takes back also while a button has the keyboard focus (reached with Tab), in the main
  window and in the EQ window.
- A speaker removed and added again in one session no longer closes an earlier change in the
  history when one of its new writes is refused.
- A history entry with some changes not written and the others taken back says so, not that the
  value before is unknown.
- Where the folder's lock file cannot be made (a folder of its name in the way, a read-only
  folder), the app writes neither log nor settings, so a second app there cannot lose them; a
  damaged settings file is then left where it is, and what loading found stays said.
- A settings file read but with values not understood and no copy kept says so, instead of
  "cannot be read".
- A serial the speaker reports in another form than text is not taken, so it cannot lock out the
  real one later.
- An EQ file with a filter line after the highest band number is refused; it could count on
  from band 1.
- A band reported with Q 0 no longer turns the EQ curve into no numbers.
- A speaker the scan heard once before and once after its serial was announced is offered once.
- "Manual could not be opened" when it is there but nothing opens it, instead of "not found";
  a wiki or release link that does not open is said too.
- A speaker remembered at a link-local address is reached also after Windows gave the network
  adapter another index (after a driver or adapter change), without a scan, which found nothing
  where mDNS gets no answer; the new index is remembered.
- A speaker that gets its serial later (added by address) keeps its history, a dim waiting to go
  back, its strip with a value being typed or dragged, and its EQ and **All values** windows as
  they are.
- The writes of a speaker that moved to another address count by the answers of its old
  connection: one that went through stays in the history as written, one still waiting there is
  not sent, and a setting such a write went through or may have gone through is read again.
- A typed value left open when its tab, panel or mode went does not come back with old text.
- **Ctrl+K** keeps to a text field being edited, and one **Escape** closes a window while a
  button has the keyboard focus.
- A write failed by an outage leaves the "unreachable" message.
- The EQ import preview and preset load say why nothing can be written (not connected,
  protected crossover) instead of "already on the speaker", also when nothing would change.
- Taking back a change sent right after another to the same setting writes the value before
  both when the speaker refused the first, not the refused value; a refused EQ band no longer
  counts as the band's value while other bands are still on their way.
- A shortcut or a dim going back while the window is minimized is kept in the history as not
  written when its write fails, not as a change to take back.
- Windows of their own open at the size they were left, also below the main window's least
  size, and never below their own.
- With own layouts, the edit note under a strip says the change is this speaker's only.
- A serial in a text of **All values** and the scan report is masked, also without a known
  serial; numbers are shown as the speaker reports them.
- A start that fails (no graphics adapter) says why in the log.

## [0.2.0] - 2026-10-27

### Added

- **Linux**: a portable build for x86_64 Linux as a `.tar.gz`, with everything but the strip with
  the system volume and the shortcuts from other programs, which Linux does not offer. Under
  Wayland the app keeps the windows' sizes, and the desktop decides where they open.
- **Groups** window, from the toolbar (instead of **+ Group**): every group in a list, adding,
  deleting and each group's settings, including what it links; the group dialog is gone, and a
  group strip's name and **Details** open this window.
- **Group** tab in the device panel: the speaker's group, that group's settings and
  **Apply to group**; the overview no longer has them.
- **Show group strip** in a group's settings: a group can do without its strip on the desk; its
  members then have their own rows in the compact mode. A member's name opens its group from its
  context menu.

### Changed

- The EQ window, the settings and the preset load dialog have no **Close** button of their own
  any more; the X in the title bar and Escape close them.
- `SHA256SUMS` on the release page lists the checksums of the downloads (the ZIP and the
  `.tar.gz`); each folder keeps its own list of its files.
- Switched to a mode without a size of its own, the window is fitted to its content right away.

### Fixed

- A window without a saved size no longer swings between two sizes for good while it is fitted
  to the desk or the compact rows (seen as the strips grew one after another as they connected).
- A window fitted to its content is no longer left a pixel short of it, with scroll bars, at
  display scales where a pixel is less than a point (125 %).
- A desk or compact window that fits its content exactly shows no scroll bars; at 125 % the
  first window of four speakers kept swapping a horizontal and a vertical one.
- A window that is still fitted to its content no longer keeps that size as if you had chosen
  it: closed (or switched to the compact mode and back) before the speakers were added, it
  stayed at its first size with scroll bars once they were.
- The held peak's line in a meter lies on the same pixels as the meter's segments; it could
  stand out beside them and look thicker or thinner from level to level.
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
