# neumann-control

A portable Windows app to find, read and control Neumann KH DSP speakers over the
Sennheiser Sound Control protocol (SSC). The user interface is in German.

This is an independent project. It is not affiliated with or endorsed by Neumann or Sennheiser.

## Status

Version 0.1.0. The protocol layer, the polling and the interface are covered by automated tests
against a built-in speaker simulator for the KH 120 II and the KH 750. The app has not yet been
verified against real speakers; the model support described below follows the protocol
knowledge the path catalog is built from and the simulator, not a hardware test.

## Features

- Finds speakers on the local network with mDNS (`_ssc._tcp`) when you press "Suchen" (a scan
  takes 4 seconds), or adds a speaker by its IPv6 address with "+ Adresse".
- One card per speaker: connection state, output level fader (0-120 dB SPL, mouse wheel in
  0.5 dB steps), input level meters with clip lamps, "Stumm" (mute) and "Ident." (identify),
  firmware version and masked serial number.
- "Mehr" opens every setting the catalog knows for that model (output, input, standby, device,
  rear panel, metering), grouped; settings a model may write get a control, the rest are shown
  greyed.
- "Alle Werte..." lists every value the speaker reports in its own schema, with a path filter and
  copy buttons, including paths the catalog does not know yet.
- Shows the control mode (NETWORK or LOCAL), which the CONTROL switch on the back of the speaker
  sets; it cannot be changed over the network. In LOCAL the speaker refuses every write.
- A speaker that stops answering is shown as unreachable and reconnected every 10 seconds.

## Supported models

| Model | Read | Write |
|-------|------|-------|
| KH 120 II | yes | yes |
| KH 750 | yes | yes, except standby (see limits) |
| Other models in the catalog (for example KH 80, KH 150) | yes | no |

The catalog is `crates/core/src/paths.toml`. A product that is not listed there for a path is
read-only, so models whose behaviour is unknown never receive a write. Settings that only exist
on some models (subwoofer outputs, logo brightness) appear only for those models.

## Requirements

- Windows 10 or 11 with a DirectX 12 capable graphics adapter.
- The computer and the speakers on the same layer 2 network segment (same switch or VLAN).
  The speakers are reached over IPv6 link-local addresses, which are not routed.
- IPv6 enabled on the network adapter in use.
- To scan, the firewall must allow mDNS (UDP 5353) for the app. Adding a speaker by address
  needs only the speaker's TCP port 45.

## Usage

1. Start `neumann-control.exe` and press "Suchen". Found speakers get a card.
2. If the scan finds nothing (for example mDNS is blocked), press "+ Adresse" and enter the
   speaker's IPv6 address. A link-local address (`fe80::...`) needs the index of the network
   adapter after a `%`, for example `fe80::1%7`.
3. Find the index in PowerShell with `Get-NetAdapter | Select-Object Name, ifIndex`, or with
   `netsh interface ipv6 show interfaces` (column `Idx`).

Manually added speakers are remembered and reconnected at every start. Speakers found by a scan
are listed for the current session only; they are not saved unless added by address.

IPv4 addresses are not accepted: the speakers are reachable only over IPv6.

## Files

The app is portable and writes only next to the executable:

| File | Content |
|------|---------|
| `settings.json` | Manually added speakers (address, port, serial) and the window position and size |
| `neumann-control.log` | Log of the current run, replaced at each start |

Speaker addresses and serial numbers are masked in the log and in the interface. An unreadable
or malformed `settings.json` is renamed to `settings.json.bad` and replaced by defaults. To
remove a manually added speaker, delete its entry from `settings.json` while the app is closed.

## Known limits

- SSC has no authentication. Anyone on the network segment can read and change the settings of
  every speaker; use the app only on a network you trust.
- A speaker in standby does not answer on the network, so it looks the same as a speaker that is
  switched off ("nicht erreichbar (aus oder Standby)").
- A speaker accepts only a few TCP connections at a time (about four). With more clients
  connected (vendor app, home automation), it can look unreachable as well.
- On the KH 750, auto-standby, standby time and standby level are shown but not written; writing
  them is untested on that model.
- Models other than the KH 120 II and the KH 750 are read-only.
- Only IPv6 link-local addressing is supported; IPv4 is not.
- Windows only.

## Building

Needs Rust 1.95 or newer.

    cargo build --release --locked -p neumann-control-app

`check.ps1` runs the quality gates (format, licence check, `cargo audit`, clippy, tests); it
needs `cargo-audit` and `cargo-about`. `build-release.ps1` runs them on a clean working tree and
writes `release\<version>\` with the executable, `LICENSE`, `THIRD-PARTY-LICENSES.html` and
`SHA256SUMS`.

## Licence

MIT, see `LICENSE`. The licences of the bundled dependencies are in
`THIRD-PARTY-LICENSES.html`; the Inter font is under the SIL Open Font Licence.
