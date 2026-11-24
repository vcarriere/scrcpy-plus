# Original-code bug review

This completes the scoped backlog 09 review of the original SCRCPY+ code, with
fixes applied to this fork. Backlog 18 remains the separate final-project audit.

Original source: Frontesque/scrcpy-plus at
`30ee624b37d28c79af03517b713f95026d388266`. Review started from fork commit
`7791fa7`. Original files were compared through Git; the installed application,
password helper, phones, and shared ADB server were not operated for this review.

## Confirmed findings and fixes

### R01 — P1: invalid download destinations and mismatched URLs

The original `downloader::download_and_extract` accepted `.` and `..` as version
strings before recursively replacing the destination. Its URL check also accepted
paths that could normalize outside the intended release directory. Invalid
components, Windows aliases/reserved names, extra URL path segments, encoded
separators, and mismatched release tags are now rejected before network access.
Regression fixtures verify that existing files remain intact.

### R02 — P1: extraction failure removed the working version

The original downloader removed the old directory before parsing/extracting or
checking the archive. A corrupt or incomplete package therefore destroyed the
working copy. It now extracts to a unique staging directory, validates executable,
server, and Windows ADB DLL files, then activates with rollback. Both current and
legacy server filenames work. A failed rollback retains and reports the recovery
copy. Downloads have a 60-second deadline and a 512 MiB compressed-byte limit.
Synthetic ZIP/TAR and injected rename/read failures cover these paths.

### R03 — P1: downloads overwrote custom executable preferences

`install_scrcpy_release` previously saved both new paths before native activation,
including a password wrapper or separately selected ADB. It now preserves existing
ADB and custom launcher selections, activates before saving, and rolls back native
and saved settings on persistence failure. A visible notice explains preserved
choices. Reinstalling a directory containing a selected executable is rejected,
so keeping its saved path cannot hide deletion of the actual custom file.

### R04 — P2: overlapping installs and tool/session operations

Downloads previously had no shared lifecycle reservation. Native installation now
rejects active sessions/ADB operations and blocks new commands, Start/Stop, and
path changes until it finishes. It preserves intentional Stop. Shared frontend
state blocks duplicate installations across route changes. This is in-process
coordination; other apps' processes are not stopped or claimed to be owned.

### R05 — P2: Windows ARM64 downloaded the x64 package

`find_scrcpy_asset` mapped every Windows architecture other than x86 to win64.
It now maps x86, x86_64, and aarch64 explicitly to win32, win64, and winarm64;
unknown architectures have no installable asset. The matching native ARM64 asset
was verified in the [official release API](https://api.github.com/repos/Genymobile/scrcpy/releases/latest).
UI fixtures cover all three choices and the unsupported case.

### R06 — P2: bundled tools depended on the launch directory

Windows defaults were relative `scrcpy/adb.exe` and `scrcpy/scrcpy.exe`. Launching
the GUI by absolute path from another directory could make bundled tools appear
missing. Defaults now resolve beside the GUI executable, consistently in status
and commands. Unix keeps its PATH lookup. Spawned tools receive the chosen ADB
path explicitly, rather than inheriting an unrelated ADB environment override.
Fixtures check paths without changing the process working directory.

### R07 — P2: version diagnostics could run a custom launcher

Startup version information ran the selected scrcpy command with `--version`,
which could execute a script/password wrapper. Diagnostics now inspect the
launcher type and skip script/non-native files. Native scrcpy/ADB output is parsed
by labels rather than fixed token positions. Unavailable metadata is displayed
honestly and a failed query leaves the home page usable. Synthetic wrapper files
prove that metadata checks do not execute them. Actual Start still uses the chosen
launcher; compiled executables' behavior is not inferred from a filename.

### R08 — P2: wireless errors could be silent or classified as success

Pairing/connect responses that exited successfully but reported failure were not
shown, and substring matching accepted text such as `not connected`. Inputs now
validate address/port and pairing-code format, requests are serialized, success
must match ADB's positive response, and unsuccessful responses show a retryable
message. The saved connection changes only after confirmed success. Tests use
synthetic endpoints and pairing codes; no real pairing was attempted.

### R09 — P2: delayed completions navigated or reloaded another page

Wireless success scheduled an unowned Back timer; a pending response could also
schedule it after leaving. The component now owns/cancels that timer and ignores
late replies. IPv6 host prefill keeps the bracketed address instead of splitting
on its first colon. The missing-ADB download action likewise checks component
lifetime before reloading. Regression cases leave during pending replies/timers
and complete an installation after navigating to Settings.

### R10 — P2: rotation lock passed a removed argument

The original option still emitted `--lock-video-orientation`. Current supported
scrcpy uses `--capture-orientation=@` to lock its initial capture orientation.
The option and locale keys now use that argument, and saved legacy flags migrate
when loaded. The launch-argument test checks the replacement and absence of the
old flag. See the [official orientation documentation](https://github.com/Genymobile/scrcpy/blob/v5.0.1/doc/video.md#orientation).
Compatibility with much older scrcpy releases remains a version-compatibility
follow-up; the existing automatic power feature still requires scrcpy 5.0.1.

### R11 — P2: bitrate was remembered even after opting out

Bitrate alone used global page state while other launch choices were local.
With Remember SCRCPY config disabled, navigating away and back retained bitrate.
It is now local configuration state, restored only through the same saved-config
logic as the other fields. A UI regression changes bitrate with remembering off
and verifies the default after navigation.

### R12 — P2: an arbitrary directory name claimed a release was active

The manager treated any path containing a version-named directory as proof that
the release was in use, including a custom wrapper. It now uses an exact managed
installation receipt and selected path. Older selections without a receipt remain
custom; ownership is not guessed from their directory names. Installation and
storage-failure tests verify that active status follows actual saved selection.

## Related upstream reports

- [#89 bundled detection](https://github.com/Frontesque/scrcpy-plus/issues/89)
  is closed upstream; R06 independently addresses the remaining working-directory
  dependency present in the reviewed source.
- [#80 GUI blocking](https://github.com/Frontesque/scrcpy-plus/issues/80),
  [#77 duplicate Start](https://github.com/Frontesque/scrcpy-plus/issues/77), and
  [#83 stuck ADB](https://github.com/Frontesque/scrcpy-plus/issues/83) were checked
  against the earlier fork work on blocking commands, session state, timeouts,
  and ADB controls. Recurring mDNS root-cause work remains backlog 13.
- [#90 macOS signing](https://github.com/Frontesque/scrcpy-plus/issues/90) was
  already addressed by the chosen upstream baseline. No new fix is claimed here.

## Verification and remaining scope

- `npm run generate` succeeded.
- All **70 headless UI tests** passed against the generated frontend with mocked
  Tauri responses. New cases exercise wireless/config/version-manager failures
  alongside the existing settings, device, ADB, and protected-screen checks.
- `cargo test --lib -- --test-threads=1`: **69 passed, 12 ignored**. The ignored
  fixtures require explicit live-device/server settings and were not enabled.
- Downloader dependencies were updated to official stable ureq 3.5.2 and zip
  9.8.2; lockfiles and MIT notices were retained. Their declared minimum Rust
  versions remain compatible with this project's Rust 1.90 minimum/1.100 tools.

This is Windows source/fixture coverage, not Linux/macOS or phone acceptance and
not a claim that every defect has been found. Real release installation, failed
activation under external file locks, power-loss recovery, cross-process locking,
and decompressed-size limits remain final-project acceptance/hardening work.
Italian/Japanese translation coverage is incomplete and uses the existing English
fallback. The original Discord button is an inactive placeholder with no verified
current invite in the repository; it needs a product decision before release.
Both are explicitly retained under backlog 18 rather than reported as fixed.

Native password/pattern unlock, independent ADB version selection/updates,
downgrade prompts, and automatic-update preference changes remain their separate
backlog tasks. This review does not implement or imply those features.
