# Development checkpoint — November 23, 2026

The first implementation adds ADB server status, a small status dot with text,
and Start/Stop controls revealed by **Show advanced ADB controls** in Settings.
It preserves normal startup: device discovery can start ADB when the main page
opens. After an explicit Stop, the native backend blocks device commands and
new mirroring sessions until an explicit Start or Resume. Page navigation does not clear
that state. Restarting the app begins a new app session.

Stop requires this app's active scrcpy sessions and current ADB commands to finish.
A confirmation explains the impact on other tools sharing the selected server.
Closing the app does not stop a shared ADB server. If another tool starts the
server while this app is paused, the status shows **Running · app paused**.
**Resume device refresh** resumes this app without replacing the running server;
the action is available on the main page as well as in Advanced controls.

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
returned to Running while the preview stayed paused. That feedback prompted the
clearer combined status and explicit Resume action. The full native/device
acceptance checklist remains open, and the installed app is unchanged. Automatic ADB updates,
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

- Eight Rust tests cover passive protocol queries, a non-ADB endpoint, closed
  ports, independent endpoints, paused-command/session guards, launch failure,
  and unsuccessful exit status.
- Seven headless Chromium tests exercise the actual built frontend with synthetic
  Tauri responses: optional controls/status text, cancellation, Stop across page
  navigation and multiple refresh cycles, Start recovery, errors/active sessions,
  offline/unauthorized states, an external restart requiring explicit Resume,
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

## Recurring wireless discovery investigation

The owner reported a second occurrence of the phone missing from the installed
app. Read-only checks found ADB 37.0.3 running with mDNS enabled, but no connected
devices and an empty mDNS service list. Connecting to the current endpoint supplied
by the owner succeeded and the device identity matched the enrolled phone. The
shared ADB server was not restarted. This restores a connection, not automatic
discovery reliability.

ADB's service list stayed empty after the direct connection. Bounded multicast
and unicast queries for `_adb-tls-connect._tcp.local` received no replies. These
observations do not establish whether the cause is the phone's advertising,
network multicast forwarding, host filtering, or ADB's discovery implementation.
Keep investigation and verified recovery under backlog item 13; do not hard-code
the owner's address/port or publish their device identity, keys, or private logs.

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

These build/test dependency licenses remain in their packages. Existing scrcpy
attribution and licensing remain in the README and backlog.
