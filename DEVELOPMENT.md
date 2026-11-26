# Development checkpoint — November 23, 2026

The first implementation adds ADB server status, a small status dot with text,
and Start/Stop controls revealed by **Show advanced controls** in Settings.
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
is unchanged. That initial preview left automatic ADB updates, native unlock,
device labels/grouping, and full child-process cancellation as backlog work.
Later checkpoints below record the implemented additions and their distinct
source, simulated, and live verification limits.

## Startup progress checkpoint — November 26, 2026

Start SCRCPY now immediately opens a themed progress panel with a rotating ring
and the actual current step. Connection validation, screen preparation, private
password entry, verification, screen-off restoration, and video startup report
coarse native phases. Manual recovery replaces the spinner with instructions in
the same panel. Progress can be hidden/reopened and survives page navigation;
a subsequent private relock/manual-recovery state can reopen it. Reduced motion
stops rotation while preserving the visible indicator and text.

`launch_progress.rs` scopes an optional launch ID to the existing owned session.
`get_scrcpy_progress` reads host memory only; polling introduces no new ADB or
phone queries. Raw child output is drained continuously with bounded lines and
is never returned or retained as progress. A decoded-video texture marker from
the owned scrcpy child confirms readiness, independently of phone unlock. Late
markers cannot override a new child, private entry, manual recovery, or a finished
session. Quiet/custom launches remain explicitly unconfirmed after ten seconds;
spawning a process alone does not close the progress panel as successful video.

The exact marker forms were checked against the official scrcpy 5.0.1
[software interop](https://github.com/Genymobile/scrcpy/blob/v5.0.1/app/src/interop/interop_software.c)
and its D3D11VA/VA-API/VideoToolbox siblings. This is an output-format reference,
not copied implementation. Existing password guards, per-character state checks,
deliberate waits, and attempt blocking are unchanged. These checks can add time
before the visible mirror opens; this change supplies progress rather than an
unmeasured speed improvement.

Verification: **141 native tests passed**, with 21 environment/opt-in tests
ignored by the default run; **127 headless UI tests passed**. Coverage includes
readiness versus process launch, bounded diagnostics, stale child output, manual
recovery, hide/reopen/navigation, launch failure/retry, late IPC replies, narrow
layout, and reduced motion. Synthetic startup/manual-recovery renders were
inspected offscreen. No desktop, phone, ADB server, saved credential, or installed
application was operated for these checks. Live startup timing/native password
acceptance remains separate from these results. The seven startup UI cases were
rechecked after the final relock, progress-query failure, and screen-state wording
changes, and passed.

Preview v20 was built from clean commit
`26deaabae91f04b136c003d849e6a19e7e5eaba5` at
`artifacts/adb-controls-preview-v20/Start Test Build.cmd`. All 18 manifest hashes
matched; the executable is an x64 Windows GUI (`0x8664`, subsystem 2), using
`me.vcarriere.scrcpy-plus.preview`. Executable SHA-256:
`69060b3b7462188c6742ca48789f2dd90eda35cf50aabbfa9016e6519c20c28e`.
Packaging did not launch it or replace the installed app. Test browser contexts
and the local test listener were closed. Backlog 19 separately records the
owner's proposed embedded mirror/overlay wrapper and the bounded feasibility
review; no wrapper implementation or benchmark was performed.

## Component update checkpoint — November 26, 2026

Update work now covers backlog 02, 07, and 15 in source. The combined checker
reports SCRCPY+, ADB, and scrcpy separately, including installed/available versions,
source failures, and unknown versions. SCRCPY+ keeps the existing Frontesque
upstream release feed and release-page download action; there is no automatic
application-binary replacement or claim of signed fork self-updates. ADB uses
Google's stable Platform-Tools repository, and scrcpy uses Genymobile releases.
The manual **Check for updates** button works with automatic checks on or off
without changing the saved preference. A source failure does not discard another
component's successful result; shared pending state prevents duplicate checks.

Advanced settings have separate ADB and scrcpy automatic-install opt-ins, both
off by default. Main automatic checking and Remember SCRCPY config remain on by
default; Advanced remains hidden. Existing saved choices retain their meaning.
The background monitor checks availability and installs only opted-in managed
tools when idle; custom executable overrides remain notify-only. Automatic scrcpy
activation also retains the currently supported 5.0.1 control compatibility rather
than silently breaking native unlock or protected-screen switching. The Android
SDK agreement is linked beside manual Platform-Tools downloads and the Advanced
ADB install opt-in. Only those deliberate actions acknowledge its terms; merely
checking for updates does not accept a license or install a package.

The shared version manager includes independent ADB selection and kept local
ADB/scrcpy packages. It does not invent older Google archive URLs: an ADB package
must come from official metadata or an already verified local installation.
Selecting an old scrcpy release resolves its official checksum file on demand
when the GitHub asset digest is absent. Package bytes are checked against the
published size and digest before bounded staging/extraction. Google currently
publishes SHA-1 in authenticated repository metadata; that legacy integrity check
is not represented as an independent publisher signature. scrcpy uses published
SHA-256 digests or SHA256SUMS. Runtime versions, required files, host architecture,
and retained ADB receipts are checked before selection. Notices remain packaged.

Native installation/activation gates reject active sessions and conflicting ADB
operations. Changing the selected client never automatically stops or restarts
the shared server. Its protocol must match the running daemon, including when
resetting to a PATH-based default. A protocol mismatch requires an explicit user
Stop before selection. Existing scrcpy version directories are not overwritten:
a valid copy can be reused offline, and an invalid copy is kept for deliberate
recovery. This also avoids replacing an executable used by a foreign daemon.

A successful manual version change disables only that tool's automatic
installation. Automatic checking stays unchanged. Numeric downgrade prompts show
current and target versions and require confirmation; an unknown version also
requires confirmation. Cancellation precedes downloading/restoring/activation.
Native paths and saved selections/update preferences use rollback if persistence
fails. scrcpy changes preserve independently selected ADB and custom launchers.

Conditional compression of inactive kept ADB packages is implemented, with
synthetic cache/restore coverage and separate full-package acceptance. Automatic
compaction runs only after native activation and saved preferences commit; cache failure
reports a warning without reverting a successful tool selection. The selected
client and running server are protected, an unknown daemon origin prevents
compaction, and platforms without equivalent loaded-file protection skip removal
of expanded packages. Each package must save at least 30% and demonstrate a
checksum-verified restore within two seconds before its expanded copy is removed.
Selecting a compressed local version restores and validates it offline after any
downgrade confirmation, before executable activation. **Compress inactive versions**
also works locally without checking release feeds, processes at most two eligible
versions per action, and reports any remaining work. Savings include cache-manifest
overhead. Managed-path aliases still require complete package validation.

The feasibility probe used only the existing v18 preview's ADB subset, not a
fresh full Google SDK package. It compressed 9,996,465 bytes to 4,572,608 bytes
(54.3% saved). Compression took 1,838/1,930/2,035 ms; checksum-verified restoration
took 618/669/636 ms across three runs. These measurements establish feasibility
for that subset, not final full-package cache or cross-platform acceptance.

Verification limits at this checkpoint:

- Public native metadata probes passed for ADB 37.0.3, scrcpy 5.0.1, and an older
  scrcpy release's SHA256SUMS fallback. They downloaded metadata/checksum text
  only, not executables, and did not contact a phone or change the shared server.
- The final native suite passed **138 tests**, with 21 environment/opt-in tests
  skipped by the default run. The headless UI suite passed **120 tests**, including
  offline kept/compressed selection, cancellation, failure/rollback, local-only
  compaction, update policies, and unchanged Advanced scroll positioning. Narrow
  Settings and downgrade-dialog renders were inspected using synthetic data.
  Playwright closed its contexts and test server; no desktop was inspected.
- A separate existing-runtime probe passed using only the v18 preview's public
  scrcpy runtime: offline discovery/validation worked without a new receipt or
  network request. Full fresh SDK installation/cache and platform acceptance remain
  pending; synthetic tests do not establish those results.
- The installed application, its existing tools, pairing keys, phone state, and
  password helper have not been changed by this update implementation. Native
  password playback acceptance from v18 remains pending local enrollment.
- Preview v19 was built from clean source commit
  `0a6caeedff00e16fa927a237a3c6f783e2e309d3` and is available at
  `artifacts/adb-controls-preview-v19/Start Test Build.cmd`. All 18 manifest file
  hashes matched. Its executable is an x64 Windows GUI (machine `0x8664`,
  subsystem 2), using the separate `me.vcarriere.scrcpy-plus.preview` profile.
  Executable SHA-256:
  `43b6b11c36a0dcaa266b781ca95988aa56d9cbcd85cf15208cb35fee0a18a4f5`.
  Packaging did not launch the app. Source, synthetic, and packaging results do
  not establish full-SDK or owner acceptance; the installed app is unchanged.

New implementation references are `tool_catalog.rs`, `adb_installer.rs`, the
verified/kept-package paths in `downloader.rs`, native activation commands in
`lib.rs`, `app_updates.ts`, `tool_updates.client.ts`, and the shared version
selection/confirmation components. quick-xml 0.42.0 supplies bounded XML parsing;
RustCrypto sha1 0.11.0 and sha2 0.11.2 supply digest implementations. Their MIT
licenses are retained in THIRD_PARTY_NOTICES.md; existing project/runtime notices
and attribution are preserved.

## Native automatic unlock checkpoint — November 25, 2026

The owner narrowed the current implementation to automatic method detection and
password unlock only. Former backlog item 04 is now integrated into item 03:
PIN and nine-dot pattern playback remain deferred within the shared method model.
The native feature is implemented in source; live password acceptance is pending.
The installed app and the existing helper are unchanged. The preview neither
reads nor migrates that helper's saved password; enrollment is local and explicit.

`automatic_unlock.rs` coordinates native commands, guarded replay, recovery, and
session lifetime. `unlock_detection.rs` reads bounded, filtered state through the
selected verified ADB transport. It distinguishes configured PIN/password/pattern/
none/unknown from the current main-display SystemUI entry surface; missing or
ambiguous state never permits input. Password replay currently requires Windows,
Android 15+ owner user 0, a local ADB server, and direct scrcpy 5.0.1 with its
matching server. The ordinary manual mirror remains available without enrollment.

`unlock_store.rs` uses Windows Credential Manager with separate OS-protected
metadata/attempt and secret records. Both bind identity, Android user, schema,
password method, and random enrollment generation. A generation change disables
the previous enrollment before replacing its secret; malformed/swapped/missing
records and interrupted writes fail closed. A per-phone lease serializes replay
and enrollment changes across application copies. Already-unlocked monitoring
reads metadata only, without retrieving the password. Native secret buffers are
zeroized on release; ordinary settings, exported profiles, diagnostics, shell
arguments, and the clipboard do not carry the password.

Replay closes and waits for this session's mirror before retrieving any secret.
The remaining scrcpy channel has video/audio/clipboard synchronization disabled
and sends native text/control messages. Android stays logically awake while its
physical panel is confirmed Off. Display-0 focus, current SystemUI password editor,
boot/owner/SIM/occlusion state, configured method, identity, and privacy are checked
before input, throughout typing, and before Enter. The attempt block is persisted
before the first character can be sent. Unknown/partial transport or verification
outcomes preserve that block across restarts until verified manual unlock.
Pre-input errors use bounded backoff; power restoration retries independently.
After credential input, the mirror reopens only after verified unlock and confirmed
physical Off restoration. A proven pre-input failure may reopen a manual mirror
without retrieving or entering the password.
Recording/capture sinks, wrappers, and unsupported flags or displays are rejected
for automatic unlock. Item 05's power monitor shares the controller and yields
during replay, rather than competing with password privacy.

The method-aware UI provides masked confirmation/enrollment, replace/remove,
explicit per-device enablement, status/recovery feedback, and a latched
**Stop session** action while the mirror is temporarily closed. Active sessions
disable credential edits. Password support is 4–64 printable ASCII characters;
PIN/pattern/unknown detections cannot silently select another credential. A known
method change persistently disables the old enrollment; only explicit password
re-enrollment can restore it, even if the configured method returns to Password.
The session owner now closes owned mirrors on GUI exit, preventing an unmanaged
mirror from surviving the privacy monitor. It does not terminate unrelated
mirrors/processes or stop the shared ADB server.

Verification at this checkpoint:

- The native suite has 106 passing tests and 16 opt-in tests excluded.
  Synthetic checks cover storage/generation/attempt invariants, method parsing,
  identity/focus/privacy guards, replay ordering, cancellation, and safe recovery.
- The opt-in Windows Credential Manager round trip passed using a unique synthetic
  binding. It verified native enrollment, enablement, attempt blocking, secret
  retrieval, method invalidation, and deletion of its own test records. It did not
  read or alter any real-device credential record.
- The frontend generated successfully. All 87 UI tests passed, including 17
  unlock tests. Old positional Display Settings selectors were repaired to
  accommodate the additional section. The narrow synthetic preview was
  inspected without accessing the owner's desktop; test browsers and listener
  were closed. These mocked results do not prove device-side unlock success.
- The connected Pixel's read-only native detector reported `PASSWORD`, SDK 37,
  Android user 0, safely locked, and logically asleep. No secret was retrieved
  or entered, and no credential verification command was used for detection.
- The non-credential private-editor preparation probe passed after the main
  power-state query was corrected to exclude duplicated numeric display-group
  wake diagnostics. The earlier ambiguity failed closed. The passing probe
  verified the stable current display-0 password editor, physical panel Off,
  and pinned control channel, then restored the original sleep/panel state.
  No secret was retrieved or entered in either probe. No native live password
  success or complete parity with the existing helper is claimed. Startup/relock,
  cold boot/reconnect, privacy transitions, interrupted input/manual recovery,
  and reopened-mirror responsiveness still need scoped real-device acceptance
  after local enrollment.

Portable preview v18 was built from clean code commit
`85c8f7b4d76fc761f88cdf3567b7e9cb0712fde5`. Its 18 manifest hashes and Windows x64
GUI executable format passed verification. The separate preview profile is
`me.vcarriere.scrcpy-plus.preview`; the installed app and helper are unchanged.
Subsequent additions are documentation and test-only code: the synthetic native
vault probe and an explicit-enrollment startup-unlock harness. They do not change
the v18 runtime. The password harness is compiled but has not been run; it requires
`SCRCPY_PLUS_TEST_PASSWORD=1`, `SCRCPY_PLUS_TEST_VIDEO=1`, explicitly selected test
executables, and a locally enrolled/enabled, unblocked password. It permits a
bounded guarded attempt, renders the reopened mirror offscreen, stops its owned
session, and restores the initially sleeping phone. An uncertain attempt remains
blocked and is never automatically retried by the harness. No password is passed
through the environment, command line, tool output, or chat.

Security limits: OS protection is not immutable storage or rollback attestation.
The app cannot exclude other authorized ADB clients, physical touch, code running
as the same Windows user, administrators, or a compromised/spoofed device/host.
Serial/manufacturer/product checks prevent accidental wrong-device use but do
not provide cryptographic device attestation. No custom encryption or biometric
bypass is introduced. Additional platforms, character sets, explicit helper
migration, and stronger device authentication remain future work in backlog 03.

## Verification before native unlock

From `application`, with the selected Node and Rust tools available:

```powershell
npm ci
npm run generate
npx playwright install chromium
npm run test:ui
Set-Location src-tauri
cargo test --lib
```

- Sixty-nine Rust tests cover passive protocol queries, a non-ADB endpoint, closed
  ports, independent endpoints, paused-command/session guards, launch failure,
  unsuccessful exit status, automatic recovery, stale observations around Stop,
  known-peer learning, cache bounds/expiry, local subnet/server restrictions,
  discovery failures, request throttling, the mDNS wire question, binary server
  metadata with unknown/private fields, malformed metadata, and client release
  parsing distinct from protocol version, secure-window/biometric parsing, unknown
  or ambiguous windows, focus races, power-transition debounce, manual overrides,
  fresh identity/protection checks, physical panel parsing, session ownership,
  cancellation of an actual owned query subprocess, physical-device grouping,
  identity/transport validation, bounded inventory parsing, ambiguous identities,
  distinct equal-model labels, checked per-route disconnect, filesystem-only
  executable resolution, missing/deleted overrides, wrapper non-execution,
  shared manual/automatic protection signals, stable visible-window snapshots,
  captured-client deadlines, and cancellation of stalled optional metadata.
- Seventy headless Chromium tests exercise the actual built frontend with synthetic
  Tauri responses: optional controls/status text, cancellation, Stop across page
  navigation and multiple refresh cycles, Start recovery, errors/active sessions,
  offline/unauthorized states, an external restart automatically restoring device
  discovery and the enabled Stop control without an extra Start request,
  client/server version/path mismatches, unavailable legacy details, a missing
  client alongside a healthy server, protected-screen check gating, stale-device
  results, visible errors, unsupported virtual/view-only sessions, automatic
  session status and Pause/Resume, saved opt-in/default migration, compact device
  labels, complete/copyable route names, route selection/disconnect, physical
  selection across endpoint changes, explicit selection after phone replacement,
  keyboard tabs, preserved unsaved config after an unresolved selection, visible
  disambiguators for equal long model names, matching device/body typography,
  narrow/high-scaling layouts, effective executable sources/reset/errors, and
  the automatic update checkbox with existing preferences and persistence,
  independent manual checks, shared in-flight requests across navigation,
  stable-release filtering, current/available/error results and retry,
  translated update controls and themed ADB indicators,
  stable advanced-toggle position/focus, collapsed-document scroll limits,
  spaced/wrapping screen-action buttons, explicit progress/timestamped results,
  and independent fresh checks while an older device response remains pending.
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

Verified development tools: Node 26.13.1 LTS and Rust 1.100.0. Nuxt is now 4.7.0,
checked against its [official stable release](https://github.com/nuxt/nuxt/releases/tag/v4.7.0)
and npm release metadata; Vue 3.5.46 and the Tauri API 2.12.5 remain current.
The default Nitro 2 build remains in use. Updated the in-scope
Nuxt, Vue, router, Tauri, and Tauri plugins to their checked stable versions and
refreshed both lockfiles. The declared minimum Rust version is now 1.90 to match
the updated Tauri dependencies; development uses the pinned 1.100.0 toolchain.
An explicit esbuild development dependency resolves the
Vite/i18n peer-dependency conflict without forcing incompatible peer resolutions.
Keep DaisyUI 4 and Tailwind 3 for the existing theme/configuration API; moving to
DaisyUI 5/Tailwind 4 is a separate UI migration.

The frontend generation succeeds with an upstream Nuxt/Nitro cache-driver import
warning. npm reports nine moderate dependency advisory entries after compatible
updates; these have not
been assessed or claimed fixed in this implementation.

On this machine the installed C++ Build Tools were present but the Windows SDK
was unavailable. Optional, untracked local development packages used:
`Microsoft.Windows.SDK.CPP` and `Microsoft.Windows.SDK.CPP.x64` 10.0.28000.2957,
under `.tools/`, with their original licenses. The environment helper recognizes
those paths without changing the system installation. These SDK packages and the
portable Node runtime are not part of the app or the repository distribution.

## Executable defaults and update-setting wording

The [original-code review](BUG_REVIEW.md) records the completed backlog 09 pass
and its source evidence. It adds staged/rollback-safe release extraction,
serialized installations, preservation of selected tools, Windows ARM64 asset
matching, executable-adjacent Windows defaults, script-safe version diagnostics,
wireless validation/lifetime fixes, rotation-flag migration, and consistent
Remember-config behavior. Full final-project review remains backlog 18.
The 69 native and 70 headless UI checks pass; live device/server tests remain
explicit opt-ins. Download dependencies now use stable ureq 3.5.2 and zip 9.8.2,
with updated lockfiles and retained MIT notices.

Settings shows each tool's effective path with bundled, system-PATH, custom, or
unavailable status. The native resolver uses the same requested/effective path
snapshot as command startup and performs filesystem checks on a blocking worker,
without executing ADB, scrcpy, or a password wrapper. An initially invalid saved
override explains the existing default fallback. A selected custom file deleted
after selection stays custom/unavailable, matching what startup will attempt.
Each tool can be reset independently; invalid saved selections remain visible
in the warning until reset or replaced. Status responses from an old page or
earlier selection are discarded.
Unix PATH candidates without execute permission bits are skipped. This path has
an additional Unix-only fixture; the current test run was Windows, so that
platform-specific fixture was not executed here.

The **Automatically check for updates** checkbox maps checked to the existing
`setting.disable_update_check=false` value and unchecked to true. Keeping that
key preserves preferences and compatibility with older previews. A missing key
still enables checks and now appears checked. English, Italian, and Japanese
labels are updated. **Check for updates** runs the same SCRCPY+ app release
check whether automatic checks are on or off, without changing the preference.
It shows progress, a newer stable release, up-to-date status, or a retryable error.
Manual and automatic checks share their pending request and result across pages;
requests have a ten-second timeout and no automatic HTTP retries. An empty or
unusable stable-release feed reports failure rather than claiming to be current.
At that earlier checkpoint, ADB/scrcpy checks and installation were deferred under
item 15. Disabled automatic checks made no background app-release request, while
manual checks remained available. The November 26 component-update checkpoint
above records the expanded checks and separate installation opt-ins.
The manual button sits alongside the automatic-check option at the normal window
width and wraps below it on narrow windows. Progress and results stay below the
controls. Its **Check for updates** label and height match the compact toggle row,
including larger text sizes.
The Settings toggle rows now have a 16-pixel gap at the default text size. All 23
focused settings/update/scroll checks pass with the tighter spacing; the earlier
full 50-test UI run also passed for the default and advanced-section changes.

Fresh profiles enable automatic update checks and Remember SCRCPY config and hide
advanced controls. The Settings checkbox and SCRCPY configuration save/load logic
both treat a missing remember preference as enabled. Explicit saved false/true
choices remain effective. These preferences and the saved SCRCPY configuration
live in the WebView's localStorage; there is no separate settings-default file.

The shared ADB status marker uses the existing theme's primary purple in both
the main-page version information and Advanced ADB controls. Running/Stopped text
remains readable independently of color; warning/error states retain their meaning.

Preview v12 was built from clean code commit `ee3fddf`. All 49 headless UI checks
passed, including manual checks with automatic checking on/off, pending requests
across navigation, and the existing advanced-panel scroll behavior. Saved English,
Italian, and Japanese previews were visually reviewed at normal/narrow widths and
larger text. All 18 portable artifact hashes and the Windows x64 GUI format passed.
The test browser/server closed; this build was not launched on the owner's desktop.

**Show advanced controls** reveals ADB management, SCRCPY/ADB executable paths,
and Manage versions together, directly below the settings-toggle card. Language,
automatic/manual update checks, and Remember SCRCPY config stay in the main card.
New settings designated as advanced belong in this shared section, following
`.cursor/rules/advanced-settings.mdc`. The legacy `setting.advanced_adb` key is
retained so existing visibility choices survive. Hiding this section preserves
the selected executables; the binary-path startup plugin still applies overrides.

The advanced section opens directly below the settings-toggle card. Inserting
the old panel above it moved the toggle by 580 pixels in the pointer case and
more in narrow/larger-text cases. The new order preserves its viewport position
and focus through pointer and Space-key enable/disable, including 125% text and
disabled browser scroll anchoring. When collapsing makes the document shorter,
the browser clamps at its normal maximum scroll position; the regression check
accepts only the closest position allowed by that limit.

The device tabs and connection identifiers now use the surrounding body text's
font family, size, weight, and line height. Their action buttons use the existing
primary/white-text treatment. Headless style comparisons and renders retain
full-name wrapping, visible disambiguators, and narrow/scaled layouts.

## Device labels and grouped connection paths

Device tabs use the model read from the device, falling back to ADB's model hint
or the connection name. Identical model names receive a short serial suffix,
extended when suffixes collide. Extremely long labels are visually ellipsized;
the accessible tab name and selected device details retain the full name. Tabs
wrap and support arrow/Home/End keys. The original card, purple active tab,
typography, and button conventions are retained.

The connection section lists each route's type, complete exact selector, state,
and selected radio control. Copy uses that original selector, and a wireless
disconnect affects only the checked route. Other paths keep the device visible.
The wireless-connect header wraps at small widths, avoiding collisions with the
tab list. Headless renders were checked at 380/600 CSS pixels and with 125% text
size plus 2x pixel density. The old long-label case was reproduced before editing.

The native inventory captures one ADB executable/server endpoint and reads
`devices -l`, retaining known-peer mDNS recovery. Only online transports are
queried, using their numeric `-t` transport ID and one fixed, whitelisted property
command per route. A physical group requires a fresh, nonplaceholder tuple of
`ro.serialno`, manufacturer, and product-device values. Model, IP, and mDNS text
are display/routing data, never identity proof. Unknown, offline, unauthorized,
and emulator identities remain separate. No identity is persisted or inferred
from a previously used address. A final listing discards replaced routes and
metadata is removed if that listing fails. Inventory is bounded to 64 routes and
a ten-second deadline with bounded per-command output/time.

Disambiguating suffixes stay visible even when a long model name is truncated.
The selected physical group survives route changes while a verified route to
that group remains. Its exact selected route is retained where available;
otherwise another online path in the same verified group is used. A different
physical identity at the same endpoint clears selection and requires a click.
An unresolved selection hides/disables launch while preserving the current
unsaved launch options, rather than destroying the configuration component.
Launch and wireless disconnect recheck the selected ADB/server and current
transport; verified identities are re-read before acting. Custom device-selector
overrides are rejected rather than selecting another phone. Supported attached
short-option values and non-selector clusters remain available for existing
configurations. Very long disambiguators use a short connection fallback.
These startup checks alone do not authorize credential input; the later native
unlock checkpoint adds independent per-input guards. Remember SCRCPY config is
still global. Native per-device enrollment binds the verified identity in Windows
Credential Manager, while the existing password helper remains unchanged.

Read-only live inventory and selection checks passed on two connected devices.
A controlled extra IP connection alongside the phone's existing mDNS path then
verified one physical entry with multiple routes. Checked disconnect removed
only that test-created IP route; the original route stayed online. Neither the
shared ADB server nor the pre-existing mirror was restarted. Physical USB plus
wireless combination testing was simulated; no USB hardware was connected for
this check. Actual identifiers and addresses stay out of public fixtures/logs.

Portable preview v8 was built from clean code commit `06e889b`. All 18 artifact
hashes and the Windows x64 GUI executable format were verified. The existing
mirror and shared ADB server were left running; the test browser listener closed.

DNS-SD's instance label has a 63-octet limit, distinct from the 255-octet DNS wire
name limit, including label-length octets. These protocol limits do not make a
technical service name an appropriate tab label. The app leaves ADB selectors
unchanged rather than truncating or applying hostname rules to DNS-SD names.
References: [RFC 6763 sections 4.1.1 and 4.2](https://www.rfc-editor.org/rfc/rfc6763.html#section-4.1.1)
and [RFC 1035 section 2.3.4](https://www.rfc-editor.org/rfc/rfc1035.html#section-2.3.4).

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
device, including its power-off-on-close setting. The native-unlock revision now
stops monitors and closes owned mirrors on GUI exit with bounded cleanup; it no
longer leaves a mirror running after its privacy monitor ends. An owned temporary
On can be restored to the Off baseline after identity/awake/state checks. No
blanket server shutdown or termination of unrelated processes is performed.
Native password monitoring shares this lifecycle/ownership model with item 05.

The manual **Check protected screen** action is a read-only snapshot using the
same visible main-display secure-window and SystemUI biometric detector as
automatic switching. Two relevant window/flag snapshots must agree; changing,
ambiguous, or unreadable state returns unknown. It captures one ADB executable
and endpoint, has a seven-second overall budget, and gives optional SDK metadata
only 250 ms. Display-help discovery is no longer part of this check; the retained
`power_commands` response field reports `not_checked`. An unflagged result is not
proof that mirroring is allowed. It does not change power or start monitoring.

The user reported an unflagged Authenticator result. Initial live probes also
reported clear while Authenticator was neither focused nor visible. After the
owner confirmed the Pixel was ready with Authenticator open, both the old
focused detector and the shared visible detector reported protection. The revised
manual path also returned secure/visible_secure_window/SDK 37. These reads did
not establish an Authenticator parser false negative; no phone input, power
change, screenshot, or credential retrieval was performed.

The two screen actions previously touched with zero horizontal spacing in the
600px layout. A wrapping flex row now provides an 8px gap (10px with larger text)
and allows multiline labels. Manual checks show a progress message and checked
time, making a snapshot distinguishable from automatic session status. Changing
device/availability clears pending UI state; an older reply cannot overwrite or
clear a newer check's progress. Visible feedback keeps the original app styling.
The owner then retried preview v11 and confirmed that Authenticator reports as
protected and the Pause/Check buttons are clearly separated. The running test
app and mirror were verified as v11. Its executable was built from clean code
commit `310429d`; all 18 artifact hashes and the Windows x64 GUI format passed.

Opt-in live tests require explicit `SCRCPY_PLUS_TEST_ADB`,
`SCRCPY_PLUS_TEST_SCRCPY`, and `SCRCPY_PLUS_TEST_DEVICE`. They are excluded from
normal checks:

- `screen_power::tests::live_display_power_cycle`, with
  `SCRCPY_PLUS_TEST_VIDEO=1`, decoded/rendered full-resolution 1440x3120 video
  through SDL's offscreen dummy/software renderer. Three complete physical
  On/Off cycles each retained advancing video during subsequent Settings/Home
  animations. FPS observations after each switch exclude the initial two
  seconds; no video frames or recordings are saved. This offscreen test does not
  exercise the owner's normal desktop GPU renderer.
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
  valid for detection. The owner subsequently retried both requested cases in
  preview v7 and confirmed that both mirrored-window cases stay responsive.
- The fixture options `SCRCPY_PLUS_TEST_RETURN_HOME=1` and
  `SCRCPY_PLUS_TEST_CANCEL_BIOMETRIC=1` perform guarded Home/Back test transitions
  only from the expected app/prompt. They are test-only and are not part of the
  production controller. No credential or Wi-Fi QR code was read or recorded.
  The video-enabled regression decodes frames in memory without saving them,
  capturing the owner's desktop, or exposing protected contents.

The live passes used ADB 37.0.3 and scrcpy 5.0.1 on the selected phone. Cross-build
and additional vendor/DRM cases are not established by these results.
Preview v7 was built from clean code commit `708d5b4`; all 18 artifact hashes and
the Windows x64 GUI format were verified. This later documentation-only
acceptance update does not change that tested executable. The installed app and
existing helper remain unchanged.

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
