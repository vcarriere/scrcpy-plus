# Development checkpoint — November 23, 2026

The first implementation adds ADB server status, a small status dot with text,
and Start/Stop controls revealed by **Show advanced ADB controls** in Settings.
It preserves normal startup: device discovery can start ADB when the main page
opens. After an explicit Stop, the native backend blocks device commands and
new mirroring sessions while ADB remains stopped. Status polling does not start
the server. When a fresh status check detects ADB running again, including a server
started by another app, the backend automatically resumes device discovery in
the existing GUI. The user does not need to restart the GUI or select Resume.

Stop requires this app's active scrcpy sessions and current ADB commands to finish.
A confirmation explains the impact on other tools sharing the selected server.
Closing the app does not stop a shared ADB server. An external restart restores
**Running** and enables **Stop ADB** when this app has no active mirror session.
Start/Stop use the original app's purple primary-button styling and 20-pixel
icons. The square Stop glyph is an original geometric SVG using the existing
icon component; the existing Play icon and its attribution remain unchanged.

Lifecycle revisions guard automatic recovery: a running observation begun before
or during a Stop operation cannot clear the stopped state after that operation.
Only a fresh successful server probe can resume the app, and it never launches
or replaces the ADB server to do so.

Status queries use the ADB server protocol directly. On Windows, the read-only
IPv4/IPv6 TCP listener tables identify closed local ports before querying them;
the development machine timed out connecting to some unused local ports instead
of returning a refusal. A listening non-ADB service is reported as unavailable,
not as a running ADB server. Explicit socket settings are respected. TCP and Unix
filesystem sockets are supported; other socket types report an unsupported error.
The displayed server number is the protocol version, not a Platform-Tools build.

Commands now report launch failures, unsuccessful exits, and timeouts. Short
commands have a ten-second bound and bounded stdout capture; long mirror sessions
do not accumulate captured output. Device refresh does not overlap itself and
distinguishes offline and unauthorized transports. Device properties from an old
selection cannot replace a newer selection. Wireless operation errors and failed
mirror launches appear in the UI.

Source checks and isolated tests are complete. A portable local Windows preview
can be built with `buildtools/Build-TestPreview.ps1`; see BUILDING.md. It has its
own app identifier/settings profile, an embedded frontend, and a double-click
launcher. Build configuration, executable format, and copied runtime hashes are
checked without taking over the owner's desktop. The owner opened the portable
preview and tested Stop followed by starting the original app: the shared server
returned to Running while the preview stayed paused. The owner's subsequent
request replaces that behavior with automatic recovery and original-style
buttons. The owner subsequently confirmed the Pixel appears in preview v3 after
wireless discovery was recovered. Source and headless checks cover the controls;
the full native/device acceptance checklist remains open, and the installed app
is unchanged. Automatic ADB updates,
native password/pattern unlock, protected-screen power handling, friendly device
labels, and physical-device grouping remain backlog work. Full child-process
cancellation on app exit also remains open under item 06.

## Verification

From `application`, with the selected Node and Rust tools available:

```powershell
npm ci
npm run generate
npx playwright install chromium
npm run test:ui
Set-Location src-tauri
cargo test --lib
```

- Seventeen Rust tests cover passive protocol queries, a non-ADB endpoint, closed
  ports, independent endpoints, paused-command/session guards, launch failure,
  unsuccessful exit status, automatic recovery, stale observations around Stop,
  known-peer learning, cache bounds/expiry, local subnet/server restrictions,
  discovery failures, request throttling, and the mDNS wire question.
- Seven headless Chromium tests exercise the actual built frontend with synthetic
  Tauri responses: optional controls/status text, cancellation, Stop across page
  navigation and multiple refresh cycles, Start recovery, errors/active sessions,
  offline/unauthorized states, an external restart automatically restoring device
  discovery and the enabled Stop control without an extra Start request,
  and a narrow layout. These are simulated backend
  states, not evidence of device-side behavior.
- `npm run test:adb` is an opt-in real ADB test. Set `SCRCPY_PLUS_TEST_ADB` to the
  full path of an existing ADB executable before running it. It uses temporary
  key directories, disables mDNS auto-connect, restricts USB selection to a
  nonexistent test device, and creates foreground servers on isolated loopback
  ports. It never stops the shared default server. Only test-owned processes and
  temporary directories are cleaned up.
- The real test passed with installed ADB 37.0.3: four simultaneous clients shared
  one server, a repeated Start reused it, two servers ran on different ports,
  a second server could not bind an occupied endpoint, and stopping one left the
  other responsive. The shared endpoint's protocol response stayed unchanged.
  Different ADB versions sharing a server have not been tested.
- Two ignored native mDNS tests require explicit `SCRCPY_PLUS_TEST_ADB` and
  `SCRCPY_PLUS_TEST_MDNS_CACHE` settings. The latter must name an isolated local
  test-cache directory. `probe_known_connected_peer` learns an already connected
  wireless peer and sends the production query; `recover_from_cached_peer`
  requires the same cache and a missing known live mDNS service to verify recovery.
  These tests contact the known local phone but do not restart ADB, change phone
  settings, read credentials, or send input. They are excluded from normal tests.

Test-created browser contexts and the local preview server close after the run.
Preview images under local `artifacts/` contain synthetic state only and are
excluded from Git.

## Toolchain and dependencies

Verified development tools: Node 26.13.1 LTS and Rust 1.100.0. Updated the in-scope
Nuxt, Vue, router, Tauri, and Tauri plugins to their checked stable versions and
refreshed both lockfiles. The declared minimum Rust version is now 1.90 to match
the updated Tauri dependencies; development uses the pinned 1.100.0 toolchain.
An explicit esbuild development dependency resolves the
Vite/i18n peer-dependency conflict without forcing incompatible peer resolutions.
Keep DaisyUI 4 and Tailwind 3 for the existing theme/configuration API; moving to
DaisyUI 5/Tailwind 4 is a separate UI migration.

The frontend generation succeeds with upstream tool warnings about the optional
Tailwind nesting plugin and a Nuxt/Nitro cache-driver import. npm still reports
eight moderate dependency advisories after compatible updates; these have not
been assessed or claimed fixed in this implementation.

On this machine the installed C++ Build Tools were present but the Windows SDK
was unavailable. Optional, untracked local development packages used:
`Microsoft.Windows.SDK.CPP` and `Microsoft.Windows.SDK.CPP.x64` 10.0.28000.2957,
under `.tools/`, with their original licenses. The environment helper recognizes
those paths without changing the system installation. These SDK packages and the
portable Node runtime are not part of the app or the repository distribution.

## Recurring wireless discovery investigation and recovery

ADB 37.0.3 (build 16887417), the latest stable release listed by Google on
November 23, 2026, exhibited the reported failure. The server stayed running with
LIBADBMDNS enabled while its mDNS service list was empty. A direct connection to
the current endpoint initially restored the transport; subsequent observations
found that transport online under its mDNS name even while service records were
missing. Server health, an online transport, and service discovery are separate.

Controlled packet captures distinguished the queries: a normal multicast PTR
question for `_adb-tls-connect._tcp.local` received no phone response in the
bounded test; a directed query to the known phone address, using source UDP port
5353, received the phone's multicast PTR/SRV/address response and immediately
repopulated ADB's service list. An earlier unicast query from an ephemeral source
port did not do this. The production Rust probe also elicited a valid multicast
response, about 163 ms after its query, with the shared ADB process unchanged.
The opt-in native recovery test subsequently passed with that peer's service
missing: the actual refresh path restored its mDNS record and online transport,
again without restarting the server. Other devices' advertised services did not
prevent recovery of the missing peer.
The underlying reason for the multicast-query difference remains unconfirmed;
this does not establish a general Windows, router, phone, or ADB defect.

The native device refresh now learns local IPv4 addresses from online IP
transports or mDNS service records matching an online transport. It stores at
most 16 addresses for 30 days in `known-mdns-peers.json` in the app's local data
profile. These are discovery hints, not physical identity or credential bindings.
No personal address, device ID, debugging port, pairing key, or credential is
hard-coded or shipped. Cache failures leave normal discovery available.

At most once every 30 seconds during device polling, missing known addresses on
an active, directly connected private/link-local subnet receive the standard
PTR question on UDP 5353. ADB's listener stays in place and handles service
resolution and authentication. The app then refreshes devices, preserving the
original results on a query failure. It never scans subnets or debugging ports,
reconnects through a stale port, changes phone settings, or restarts the server
to recover mDNS. The existing native operation guard also blocks this path after
intentional Stop. Passive status checks do not run it. Remote ADB servers are
excluded because their local network is not the GUI machine's network.

This fallback needs a previously learned address and a reachable phone on the
same IPv4 subnet. First connections, changed IPs, IPv6-only paths, and networks
that also block directed mDNS can still require manual connection. Repeated
GUI cold-start/outage tests and the multicast-path root cause remain open in
backlog item 13. Packet captures and actual device identifiers remain private.

## Implementation references and attribution

The fork retains Frontesque's original code and GPL-3.0-only license. New status,
control, and test code in this change is original fork work. Protocol/API
references (not copied implementations):

- [AOSP ADB client](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/client/adb_client.cpp),
  including `host:version` and `host:kill`.
- [ADB command-line documentation](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/docs/user/adb.1.md),
  including server endpoints and `--one-device`.
- Microsoft's [GetTcpTable2](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-gettcptable2)
  and [GetTcp6Table2](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-gettcp6table2).
- [windows-sys / windows-rs](https://github.com/microsoft/windows-rs), Microsoft
  and contributors, MIT OR Apache-2.0, provides the Windows API bindings.
- [Playwright](https://github.com/microsoft/playwright), Microsoft and
  contributors, Apache-2.0, provides the headless UI test tools.
- [socket2](https://github.com/rust-lang/socket2), the Rust project contributors,
  MIT OR Apache-2.0, supplies the shared UDP socket. Version 0.6.5 was checked
  against its official package release metadata.
- [if-addrs](https://github.com/messense/if-addrs), MaidSafe.net Limited and
  contributors, MIT OR BSD-3-Clause, supplies active interface/subnet information.
  Version 0.15.0 was checked against its official package release metadata.
- Google's [Platform-Tools release notes](https://developer.android.com/tools/releases/platform-tools)
  and [ADB documentation](https://developer.android.com/tools/adb).

Dependency license files remain in their packages. The newly added discovery
libraries' MIT notices are also retained in THIRD_PARTY_NOTICES.md and copied
into the portable preview. Existing scrcpy attribution and licensing remain in
the README and backlog.
