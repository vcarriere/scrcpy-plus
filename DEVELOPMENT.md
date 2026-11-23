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
Advanced controls show the selected client's effective executable path and
release/build separately from the running server's executable and release/build,
the server endpoint, and the protocol version. The client runs only `--version`
when the panel opens or its selection changes. Server build details use the
passive `host:server-status` protocol request, never an ADB client invocation.
Unsupported legacy details remain unavailable without changing Running status.
Only the public version, build, and executable fields are decoded; key-store/log
locations are discarded. A mismatch explains that Start does not replace a
running server and Stop affects other apps using it. The app never claims it
owns a server merely because its binary matches the selected executable.

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
native password/pattern unlock, friendly device
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

- Thirty-four Rust tests cover passive protocol queries, a non-ADB endpoint, closed
  ports, independent endpoints, paused-command/session guards, launch failure,
  unsuccessful exit status, automatic recovery, stale observations around Stop,
  known-peer learning, cache bounds/expiry, local subnet/server restrictions,
  discovery failures, request throttling, the mDNS wire question, binary server
  metadata with unknown/private fields, malformed metadata, and client release
  parsing distinct from protocol version, secure-window/biometric parsing, unknown
  or ambiguous windows, focus races, power-transition debounce, manual overrides,
  fresh identity/protection checks, physical panel parsing, session ownership,
  and cancellation of an actual owned query subprocess.
- Fifteen headless Chromium tests exercise the actual built frontend with synthetic
  Tauri responses: optional controls/status text, cancellation, Stop across page
  navigation and multiple refresh cycles, Start recovery, errors/active sessions,
  offline/unauthorized states, an external restart automatically restoring device
  discovery and the enabled Stop control without an extra Start request,
  client/server version/path mismatches, unavailable legacy details, a missing
  client alongside a healthy server, protected-screen check gating, stale-device
  results, visible errors, unsupported virtual/view-only sessions, automatic
  session status and Pause/Resume, saved opt-in/default migration, and a narrow layout.
  These are simulated backend
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
- An ignored passive metadata test accepts `SCRCPY_PLUS_TEST_ADB_SOCKET`. It
  passed against the existing local 37.0.3 server, reading its actual release,
  build, and executable through the native protocol with the server process
  unchanged. It does not launch an ADB client or start/stop the server.
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

## Automatic screen power for protected windows and fingerprint prompts

**Temporarily turn on phone screen for protected screens** is an opt-in checkbox
immediately below **Turn off phone screen**, using the original checkbox/button
styles. It applies when a new mirror starts and can be remembered with SCRCPY
config. Legacy configurations default to off. Virtual/other selected displays,
view-only, OTG, video-disabled sessions, device-selector overrides, and disabled
cleanup are rejected or disabled. Automatic mode requires the direct scrcpy
executable; launcher/password wrappers are not assumed to own the real session.
The existing helper and installed app are unchanged.

The native controller reads visible main-display windows. A complete secure
window flag or a visible, correctly owned/typed SystemUI BiometricPrompt can
request physical On, even if the prompt overlays another focused app. Entry is
prompt; exit requires one second of confirmed clear observations, restarted by
unknown/protected observations. Power changes recheck device serial, current
Android user, fresh protection, logical wakefulness, and actual panel state.
It never decides from black video pixels and does not unblock protected capture.
Secure surfaces/policies without an observable window signal, vendor/keyguard
variants, and multiple physical panels still need broader compatibility work.

Physical power is read from the filtered `SurfaceFlinger --displays` power modes,
excluding virtual displays, rather than DisplayManager's cached screen state.
Preview v6 used framework `cmd display power-off` / `power-reset` operations.
Its panel-only tests passed, but the owner reproduced a frozen mirror after
switching power. The mirror process and its connections remained alive and ADB
responded; the exact encoder/renderer failure was not established. Those earlier
tests used `--no-window` without a video sink, which disables video and could not
verify continued mirroring.

Power writes now use scrcpy 5.0.1's own `SET_DISPLAY_POWER` control message and
SurfaceControl implementation. An additional, uniquely named control-only
scrcpy-server instance shares the selected device and captured ADB endpoint.
It has no video/audio capture, clipboard synchronization, startup wake, or exit
power restoration. It does not reset the framework display or recreate the main
mirror. Its socket forwarding rule, Android jar path, and subprocess are owned
by the automatic session; cleanup removes only matching owned resources and
skips ADB commands if the shared server has stopped. A failed control write
closes the stream rather than reusing a potentially partial protocol message.

Automatic mode currently requires Android 15+, a readable single physical panel,
a local ADB server, and the verified scrcpy 5.0.1 executable with its matching
`scrcpy-server` alongside it (or `SCRCPY_SERVER_PATH`). Normal mirroring remains
available with other versions. Expanding protocol/version support requires
checking that release's private control protocol, not guessing compatibility.

Each automatic session captures its ADB executable/server endpoint, owns the
actual scrcpy child, and keeps the native session/Stop gate until it ends. Only
one controller may own a physical serial across routes. Queries have bounded
output/time and observe child exit, GUI shutdown, and explicit Pause; cancellation
kills only the owned short client, never the shared server. Sleep or external
panel changes pause control instead of fighting the owner. Pause leaves the
current panel under manual control; Resume explicitly authorizes control again.
Unreadable state/power failures pause with visible status and a recovery action.
Raw window dumps, app names, and hardware identity stay out of frontend status
and logs; only the already selected transport identifies its session in the GUI.

Closing a mirror ends its monitor and lets normal scrcpy cleanup restore the
device, including its power-off-on-close setting. Closing the GUI stops monitors
with a bounded wait; an owned temporary On can be restored to the Off baseline
after identity/awake/state checks. Existing mirror windows can remain open as
before, but automatic switching ends with the GUI. No blanket server shutdown or
termination of unrelated processes is performed. Native password monitoring must
reuse this lifecycle/ownership model when item 03 is implemented.

The manual **Check protected screen** action remains a read-only diagnostic of
the focused window. It returns secure/unflagged/unknown, rechecks focus, and
discards stale-device responses. An unflagged result is not proof that mirroring
is allowed. It does not change power or start monitoring.

Opt-in live tests require explicit `SCRCPY_PLUS_TEST_ADB`,
`SCRCPY_PLUS_TEST_SCRCPY`, and `SCRCPY_PLUS_TEST_DEVICE`. They are excluded from
normal checks:

- `screen_power::tests::live_display_power_cycle`, with
  `SCRCPY_PLUS_TEST_VIDEO=1`, decoded/rendered full-resolution 1440x3120 video
  through SDL's offscreen dummy/software renderer. Three complete physical
  On/Off cycles each retained advancing video during subsequent Settings/Home
  animations. FPS observations after each switch exclude the initial two
  seconds; no video frames or recordings are saved. This does not exercise the
  owner's normal desktop GPU renderer, so the new preview still needs that retry.
- `screen_power::tests::live_automatic_session`, with a bounded duration, ran the
  actual controller. Authenticator entry turned the panel On; returning Home
  restored Off. Wi-Fi sharing's visible fingerprint prompt also turned it On;
  cancelling that prompt restored Off without authenticating. Both were verified
  by the actual panel mode, not a simulated backend or an owner's visual report.
  The shared ADB process stayed running and test-owned mirrors exited.
- After replacing the power writes, the automatic Authenticator/Home test passed
  again with video enabled at 1440x3120. Both automatic panel transitions were
  observed, followed by advancing decoded/rendered video during Settings/Home
  animations after screen-off. The earlier Wi-Fi biometric panel test remains
  valid for detection; the new desktop-renderer retry must cover that case too.
- The fixture options `SCRCPY_PLUS_TEST_RETURN_HOME=1` and
  `SCRCPY_PLUS_TEST_CANCEL_BIOMETRIC=1` perform guarded Home/Back test transitions
  only from the expected app/prompt. They are test-only and are not part of the
  production controller. No credential or Wi-Fi QR code was read or recorded.
  The video-enabled regression decodes frames in memory without saving them,
  capturing the owner's desktop, or exposing protected contents.

The live passes used ADB 37.0.3 and scrcpy 5.0.1 on the selected phone. Cross-build
and additional vendor/DRM cases are not established by these results.

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
- [ADB host status schema](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/proto/adb_host.proto),
  used as the protocol reference for version/build/executable field numbers.
  The bounded decoder is original fork code; no generated AOSP source is copied.
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
- Android's [FLAG_SECURE documentation](https://developer.android.com/reference/android/view/WindowManager.LayoutParams#FLAG_SECURE),
  [WindowState](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/services/core/java/com/android/server/wm/WindowState.java),
  [WindowManagerService](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/services/core/java/com/android/server/wm/WindowManagerService.java),
  and [DisplayManagerShellCommand](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/services/core/java/com/android/server/display/DisplayManagerShellCommand.java)
  are API/output references, not copied implementation code.
- [scrcpy physical screen controls](https://github.com/Genymobile/scrcpy/blob/master/doc/device.md#turn-screen-off).
- Romain Vimont / Genymobile and scrcpy contributors' Apache-2.0
  [5.0.1 control protocol](https://github.com/Genymobile/scrcpy/blob/v5.0.1/app/src/control_msg.c),
  [server connection](https://github.com/Genymobile/scrcpy/blob/v5.0.1/server/src/main/java/com/genymobile/scrcpy/device/DesktopConnection.java),
  and [controller](https://github.com/Genymobile/scrcpy/blob/v5.0.1/server/src/main/java/com/genymobile/scrcpy/control/Controller.java)
  are references for the original Rust control client. The existing bundled
  scrcpy-server is reused without modification; its runtime license is retained.
- [SurfaceFlinger display dumps](https://android.googlesource.com/platform/frameworks/native/+/refs/heads/android16-release/services/surfaceflinger/SurfaceFlinger.cpp)
  and [SystemUI biometric prompt window](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/packages/SystemUI/src/com/android/systemui/biometrics/AuthContainerView.java)
  are output/layout references. Controller and parser code are original fork work.

Dependency license files remain in their packages. The newly added discovery
libraries' MIT notices are also retained in THIRD_PARTY_NOTICES.md and copied
into the portable preview. Existing scrcpy attribution and licensing remain in
the README and backlog.
