# SCRCPY+ additions and fixes

Started November 23, 2026. This is a planning backlog; none of these items is
implemented in this fork yet. Priorities below are proposed implementation order.

Fork: [vcarriere/scrcpy-plus](https://github.com/vcarriere/scrcpy-plus).
Upstream: [Frontesque/scrcpy-plus](https://github.com/Frontesque/scrcpy-plus),
`main` at `30ee624b37d28c79af03517b713f95026d388266` (November 8, 2026).
The source package identifies itself as version 3.1.0. The existing installed app
and owner-specific password helper remain separate from this new checkout.

## Requested additions

### 01. Advanced ADB status and Start / Stop controls

Priority: first. Feasibility: supported ADB operations; status and lifecycle need
new backend code.

- [ ] Add an **Advanced** setting that reveals an **ADB** management section.
      Keep the normal launch page simple.
- [ ] Show whether the host ADB server is **Running**, **Stopped**, **Starting**,
      **Stopping**, or **Unavailable / Error**. Distinguish an installed ADB
      executable from a running server and from connected/authorized devices.
- [ ] Show the selected ADB path and client version; include server version and
      endpoint when available. Explain conflicts with a server started by another
      tool instead of presenting every ADB process as ours.
- [ ] Add **Start ADB** (`start-server`) and **Stop ADB** (`kill-server`), with
      progress, bounded timeouts, useful errors, and disabled duplicate actions.
- [ ] Probe status without starting or restarting ADB. Evaluate a direct ADB
      server protocol query to the configured endpoint; do not use `adb devices`
      as a supposedly passive status probe. Respect explicit socket/port settings.
- [ ] Make Stop persist for the current app session: pause device polling,
      reconnects, and authentication/power monitors before stopping. Do not
      silently restart it on the next refresh. An explicit Start or launch action
      can offer to resume normal operation.
- [ ] Explain in the Stop action that a shared server may also disconnect Android
      Studio, other scrcpy sessions, and other tools. Do not stop an unrelated
      server automatically when closing SCRCPY+.

Acceptance: status refresh leaves a stopped server stopped; Start is idempotent;
Stop remains effective beyond several refresh cycles; errors and external servers
are represented accurately. Test with an isolated server endpoint before any
shared-server test.

Source starting points: `application/app/pages/settings.vue`,
`application/app/components/adb_check.vue`,
`application/app/components/devices.vue`,
`application/src-tauri/src/system_command.rs`, and
`application/src-tauri/src/lib.rs`.

### 02. Automatic ADB updates, independently of scrcpy

Priority: next, after item 01. Feasibility: practical; requires a dedicated updater.

- [ ] Add automatic stable-version checks, **Check now**, installed/available
      versions, and a setting to enable automatic installation when idle.
- [ ] Use Google's official SDK Platform-Tools downloads and release metadata.
      Check the package revision/build as well as the ADB protocol version:
      `1.0.41` alone does not identify the Platform-Tools release.
- [ ] Stage and validate the matching OS/architecture package before activating
      it. Preserve the complete required runtime set, including both ADB DLLs on
      Windows and applicable notices. Reject unsupported architectures clearly.
- [ ] Verify the official download origin and available publisher signatures or
      official integrity metadata. A locally computed hash records received bytes;
      it is not independent proof of authenticity.
- [ ] Defer activation while mirroring, unlock attempts, or other affected ADB
      work is active. Do not automatically kill the shared server or replace
      in-use binaries. Keep the previous working version for rollback.
- [ ] Preserve pairing/authorization keys, saved connections, custom paths, and
      unlock settings. Treat an externally managed custom ADB as notify-only
      unless the user explicitly chooses managed updates.
- [ ] Keep ADB updates separate from scrcpy downloads so selecting a scrcpy
      release does not silently downgrade ADB or replace a custom launcher.
- [ ] Cover offline checks, interrupted downloads, extraction failures, locked
      files, unsuccessful activation, rollback, and restoration after restart.

Acceptance: update a managed ADB installation without changing scrcpy, credentials,
pairing keys, or active sessions; failed activation retains the working version.
Google's release notes list **37.0.3 (November 2026)** at backlog creation; discover
the latest stable version at implementation time instead of hard-coding this.

Source starting points: `application/src-tauri/src/downloader.rs`,
`application/app/composables/scrcpy_release.ts`, and item 01's lifecycle code.

### 03. Integrate the existing password implementation into SCRCPY+

Priority: next, after reliable session ownership (item 06). Feasibility: existing
local helper provides a reference; the generalized native implementation is new.

- [ ] Move session startup unlock and active-session re-unlock into the native
      backend, with an opt-in per-device setting and password enrollment UI.
- [ ] Keep credentials in OS-protected storage. Never put them in source,
      ordinary settings/localStorage, command-line arguments, diagnostics, or
      exported profiles. Keep decryption and injection in the native backend;
      minimize credential lifetime in memory.
- [ ] Bind each credential to verified device identity. An IP address or changing
      wireless endpoint identifies a connection, not the enrolled phone.
- [ ] Port the existing helper's guards: check lock state on the main display,
      current Android user, focused SystemUI window, and active password editor;
      do not mistake the fingerprint panel for a password field. Recheck before
      input, during typing, and before submission.
- [ ] Skip credential retrieval and input when already unlocked. Disable automatic
      authentication for view-only/no-control, OTG, ambiguous devices, unrelated
      displays, and unsupported lock screens.
- [ ] Preserve the distinction between retryable failures before input and an
      uncertain/partial credential attempt. Persist the latter block across
      restarts; clear it only after verified manual unlock. Offer a visible
      manual-unlock prompt without repeatedly submitting a password.
- [ ] Tie monitoring and in-flight input to the actual session lifetime. Serialize
      attempts across concurrent sessions; closing the session must stop its
      automation. Coordinate screen power with item 05.
- [ ] Offer local migration from the existing helper only after native parity is
      proven. Preserve the working helper until that transition is chosen.
- [ ] Start with the existing helper's documented 4–64 printable ASCII support;
      specify broader character/PIN support separately rather than implying it
      already works. Never convert the owner's password into a PIN assumption.

Acceptance: synthetic regression fixtures cover already-unlocked, locked startup,
mid-session relock, wrong device at the same IP, fingerprint panel, display-0
selection, partial input, reconnect, blocked retry, manual recovery, and session
closure. Actual device parity requires a separately scoped live test.

Source starting points: `run_scrcpy` in `application/src-tauri/src/lib.rs`,
`application/src-tauri/src/system_command.rs`, and the existing local helper's
unlock, session, and process-lifetime code. Do not copy its personal device IDs,
credentials, or logs into the public fork.

### 04. Android nine-dot pattern unlock

Priority: after password integration. Feasibility: plausible; gesture transport
and lock-screen recognition need a prototype.

- [ ] Add **Pattern** as a separate per-device credential type with a 3×3
      enrollment grid, confirmation, reset, and protected storage.
- [ ] Submit one continuous touch gesture: down, movement through the selected
      dots, then up. Separate taps or independent swipes are insufficient.
- [ ] Validate Android's intermediate-dot rules, timing, touch cancellation,
      rotation, display insets, and varying grid positions using synthetic paths.
      Keep gesture coordinates in device-display space, independent of the
      desktop preview's scale/crop.
- [ ] Require a verified pattern entry surface. A password-editor guard cannot be
      reused unchanged. Abort on a changed lock screen or device identity.
- [ ] Investigate scrcpy's touch/control transport first. If this requires changes
      to scrcpy itself, track those in a separate upstream-derived component with
      its source and license notices, rather than claiming a GUI toggle is enough.
- [ ] Reuse item 03's attempt serialization, persistent uncertainty block, manual
      recovery, and session lifetime. Never automatically guess another pattern.

Acceptance: validate complete gesture sequences in an isolated harness; then test
only an explicitly enrolled device in a scoped live test. Distinguish manual
click-and-drag support from saved automatic pattern unlock.

### 05. Temporarily turn on the phone screen for protected screens

Priority: feasibility investigation before implementation. The likely Android
term is a **secure window** (`FLAG_SECURE`); lock-screen/keyguard state and physical
display power are separate signals.

- [ ] Immediately below **Turn off phone screen**, add a dependent, opt-in option:
      **Temporarily turn on phone screen for protected screens**.
      Disable it unless the parent option is selected.
- [ ] Explain the intended behavior: turn the physical panel on when entering a
      detected protected screen, then restore screen-off after leaving it. Turning
      the panel on does **not** remove Android screenshot/mirroring protection.
- [ ] First establish whether the focused window's secure state can be observed
      reliably with the available shell permissions on supported Android versions.
      No reliable generic signal is established by this backlog. A black frame
      alone is not proof of a protected screen.
- [ ] If automatic detection is unavailable, mark it unsupported and offer manual
      screen power. Consider clearly labelled user-selected app rules separately;
      app identity is not equivalent to detecting a secure window.
- [ ] Use explicit physical display on/off operations, not a blind power-key
      toggle. Capability-check the selected Android version and display. Upstream
      scrcpy documents `cmd display power-on/off` for Android 15 and later;
      older versions need a validated compatible implementation.
- [ ] Use one session power controller for this feature and password recovery.
      Suspend screen-off restoration during the temporary-on state; prevent
      flicker on rapid transitions and restore only changes owned by this feature.
      Respect manual overrides, disconnects, and session closure.
- [ ] Check the actual protected-app cases separately from keyguard, biometrics,
      device sleep, and DRM surfaces. Do not present this as a secure-content
      capture bypass or require rooting as part of this option.

Acceptance: demonstrate supported entry/exit detection and panel restoration
without repeated toggling or conflicts with re-unlock. Record separately whether
the protected content remains blocked in the mirror, as Android may require.

Source starting points: `application/app/components/scrcpy_config.vue`, English,
Italian, and Japanese locale files, and the session lifecycle from items 03/06.

## Supporting fixes found while inspecting upstream

### 06. Preserve process results and own the real session lifecycle

- [ ] `system_command::execute` currently returns only stdout, discards stderr
      and exit status, and returns an empty string for launch failure.
      `run_scrcpy` consequently cannot report an ordinary failed launch correctly.
      Return typed success/failure results with bounded, redacted diagnostics.
- [ ] Add session identifiers, actual child-process ownership, exit notifications,
      and cancellation so authentication/power tasks cannot outlive their session.
      Avoid holding unbounded captured output for long-running sessions.

This is a prerequisite for dependable native unlock and useful ADB error status.

### 07. Make updates preserve working tools and overrides

- [ ] `install_scrcpy_release` currently overwrites both saved binary paths.
      Preserve explicit custom tools and native unlock preferences; make changes
      deliberate and reversible.
- [ ] `download_and_extract` currently removes an existing version directory
      before extraction. Stage and validate first, then activate; an extraction
      failure must not destroy the working copy.

These are source-confirmed implementation behaviors, not claims of a reproduced
failure in the installed app. Fold the fixes into item 02 where practical.

### 08. Keep device state separate from daemon state

- [ ] `devices.vue` currently refreshes with `adb devices` every five seconds and
      infers unauthorized status from missing device properties. Parse explicit
      `device`, `offline`, and `unauthorized` states and expose them separately
      from whether the ADB server is running.
- [ ] Gate all refresh/reconnect paths on the intentional-stop state from item 01,
      including page remounts. Avoid overlapping slow refreshes.

## Delivery order and verification

1. ADB status/controls, polling semantics, and typed process/session results
   (01, 06, 08).
2. Independent ADB update staging, activation, rollback, and override preservation
   (02, 07).
3. Native password parity with the existing helper (03).
4. Pattern input prototype and protected-screen detection investigation (04, 05).

Use synthetic credentials and isolated ADB endpoints for automated checks. Keep
device-specific testing explicit. Before changing code, recheck official stable
toolchain/dependency releases and update in-scope pins/lockfiles where compatible.
This documentation-only fork does not change dependency versions or operate ADB.

For UI work, preserve existing branding and layout; keep advanced controls out of
the normal path, place the screen option beside its parent, and make state and
effects clear. Review a focused preview before packaging.

## Sources and attribution

- [Frontesque/scrcpy-plus source baseline](https://github.com/Frontesque/scrcpy-plus/tree/30ee624b37d28c79af03517b713f95026d388266),
  by Frontesque and contributors; GPL-3.0-only as declared in `Cargo.toml`.
  The original [LICENSE](LICENSE) and README credits are retained.
- [Genymobile/scrcpy](https://github.com/Genymobile/scrcpy), by Romain Vimont and
  contributors; Apache-2.0. No scrcpy source or helper implementation was added
  to this fork in this planning pass.
- [Android SDK Platform-Tools releases and official downloads](https://developer.android.com/tools/releases/platform-tools).
- [Android Debug Bridge documentation](https://developer.android.com/tools/adb),
  including server start/stop and commands that can automatically start a server.
- [Android secure-window flag](https://developer.android.com/reference/android/view/WindowManager.LayoutParams#FLAG_SECURE),
  which prevents screenshot/non-secure-display capture of protected content.
- [scrcpy physical screen power controls](https://github.com/Genymobile/scrcpy/blob/master/doc/device.md#turn-screen-off).
- [scrcpy mouse input modes](https://github.com/Genymobile/scrcpy/blob/master/doc/mouse.md).

Recheck the licenses and distribution terms for any tools or third-party code
actually incorporated in later work, and retain their required notices.
