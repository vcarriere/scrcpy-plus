# SCRCPY+ additions and fixes

Started November 23, 2026. Implementation has started on branch
`codex/adb-controls`; checked items describe source work with the verification
and limitations recorded in [DEVELOPMENT.md](DEVELOPMENT.md). The installed app
has not been replaced. Priorities below are proposed implementation order.

Fork: [vcarriere/scrcpy-plus](https://github.com/vcarriere/scrcpy-plus).
Upstream: [Frontesque/scrcpy-plus](https://github.com/Frontesque/scrcpy-plus),
`main` at `30ee624b37d28c79af03517b713f95026d388266` (November 8, 2026).
The source package identifies itself as version 3.1.0. The existing installed app
and owner-specific password helper remain separate from this new checkout.

## Requested additions

### 01. Advanced ADB status and Start / Stop controls

Priority: first. Feasibility: supported ADB operations; status and lifecycle need
new backend code.

- [x] Add an **Advanced** setting that reveals an **ADB** management section.
      Keep the normal launch page simple.
- [x] Show whether the host ADB server is **Running**, **Stopped**, **Starting**,
      **Stopping**, or **Unavailable / Error**. Distinguish an installed ADB
      executable from a running server and from connected/authorized devices.
- [x] Add a compact visual running/stopped indicator beside the ADB label, using
      a dot, square, or badge that fits the app's existing style. Keep the basic
      status easy to see, with detailed controls in Advanced. Include a text
      label or accessible description so status does not depend on color alone;
      reflect transitions and errors without implying a device is connected.
- [x] Show the selected ADB path and client version; include server version and
      endpoint when available. Explain conflicts with a server started by another
      tool instead of presenting every ADB process as ours.
- [x] Add **Start ADB** (`start-server`) and **Stop ADB** (the equivalent
      `host:kill` server request), with
      progress, bounded timeouts, useful errors, and disabled duplicate actions.
- [x] Probe status without starting or restarting ADB. Use a direct ADB
      server protocol query to the configured endpoint; do not use `adb devices`
      as a supposedly passive status probe. Respect explicit socket/port settings.
- [x] Require this app's current scrcpy sessions and ADB operations to finish
      before Stop. Pause device polling and new device commands while the server
      remains stopped. Status refresh must not restart the server. When a fresh
      check detects that another tool has started ADB again, automatically resume
      device discovery in place and enable Stop for the running server; do not ask
      for a GUI restart or manual Resume. Future native authentication/power
      monitors must use the same lifecycle gate.
- [x] Explain in the Stop action that a shared server may also disconnect Android
      Studio, other scrcpy sessions, and other tools. Do not stop an unrelated
      server automatically when closing SCRCPY+.

Acceptance: status refresh leaves a stopped server stopped; Start is idempotent;
Stop remains effective beyond several refresh cycles while no other tool starts
ADB. An externally restarted server restores device refresh automatically.
Errors and external servers are represented accurately. The visual indicator matches the observed server
state and remains understandable without color. Test with an isolated server
endpoint before any shared-server test.

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

Implemented for supported Android 15+ main-display sessions with scrcpy 5.0.1,
with live entry/exit tests on the owner's Android 17 phone. Android secure windows (`FLAG_SECURE`),
biometric overlays, logical sleep, and physical panel power are separate signals.

- [x] Start a manual, read-only **Check protected screen** action directly below
      **Turn off phone screen**, using the existing app's styling. Inspect only
      the selected device's focused main-display window and return secure flag,
      unflagged, or unknown. Recheck focus, reject ambiguous/other displays, and
      discard results after selection changes. It does not change screen power.
      The owner selected apps that show black video in scrcpy as the first case.
- [x] Immediately below **Turn off phone screen**, add a dependent, opt-in option:
      **Temporarily turn on phone screen for protected screens**.
      Disable it unless the parent option is selected.
- [x] Explain the intended behavior: turn the physical panel on when entering a
      detected protected screen, then restore screen-off after leaving it. Turning
      the panel on does **not** remove Android screenshot/mirroring protection.
- [x] Establish whether secure app windows and fingerprint prompts can be observed
      reliably with the available shell permissions on supported Android versions.
      The automatic controller inspects visible main-display secure windows and
      verified SystemUI biometric overlays, including overlays above another
      focused app. This is not a universal secure-surface detector. An unflagged window can
      still contain protected surfaces or be affected by device policy. A black
      frame alone is not proof of a protected screen.
- [x] Report unsupported capabilities and unreadable state clearly. Pause on
      uncertainty instead of guessing; retain original scrcpy manual screen-power
      controls and add Pause/Resume for the automatic controller. App rules are
      not treated as proof of a secure window.
- [x] Use explicit physical display on/off operations, not a blind power-key
      toggle. Capability-check the selected Android version and display. Replace
      v6's framework power-off/reset writes with scrcpy's own display-power
      protocol through an owned control-only server. Verify SurfaceFlinger's
      actual panel state. Automatic mode requires a local ADB server and matching
      scrcpy 5.0.1/server; other releases need validated protocol support.
- [x] Use one session power controller for this feature; future native password
      recovery must use the same ownership/lifecycle gate.
      Suspend screen-off restoration during the temporary-on state; prevent
      flicker on rapid transitions and restore only changes owned by this feature.
      Respect manual overrides, disconnects, and session closure.
- [x] Test the requested cases separately: Authenticator entry/return Home and
      Wi-Fi sharing's fingerprint prompt/cancellation both produced verified
      physical On then Off transitions. Authentication and credential input were
      not needed. This is not a secure-content capture bypass and needs no root.
- [ ] Broaden compatibility checks to other Android builds, foldable/multiple
      physical displays, keyguard-specific biometric variants, and protected
      surfaces without an observable secure-window flag. Unsupported cases must
      remain explicit rather than using black pixels as proof.
- [ ] Confirm on the owner's normal desktop renderer that the v7 power-control
      change resolves the reported v6 frozen mirror. Three full-resolution
      offscreen video On/Off cycles passed; a panel-only test is insufficient.

Acceptance: demonstrate supported entry/exit detection and panel restoration
without repeated toggling or conflicts with re-unlock. Record separately whether
the protected content remains blocked in the mirror, as Android may require.

Source starting points: `application/app/components/scrcpy_config.vue`, English,
Italian, and Japanese locale files, `protected_screen_check.vue`, native
`protected_screen.rs`, and the session lifecycle from items 03/06. Synthetic
secure/unflagged/unknown, focus changes, other windows, and display restrictions
are covered. Native `screen_power.rs` owns each automatic session and its
cancellable queries. The original panel-only checks passed both requested cases
with ADB 37.0.3 and scrcpy 5.0.1. The revised power path also passed three
full-resolution offscreen video cycles and an automatic Authenticator/Home
transition with video continuing afterward. No video is saved; the feature does
not remove Android's mirror blocking. Desktop-renderer acceptance remains open.

## Supporting fixes found while inspecting upstream

### 06. Preserve process results and own the real session lifecycle

- [x] Replace stdout-only command results with typed success/failure results.
      Report unsuccessful launches, nonzero exits, and short-command timeouts;
      bound captured stdout and avoid accumulating long mirror-session output.
      Keep diagnostics coarse rather than echoing potentially sensitive arguments.
- [ ] Add session identifiers, actual child-process ownership, exit notifications,
      and cancellation so authentication/power tasks cannot outlive their session.
      Avoid holding unbounded captured output for long-running sessions.
      Automatic screen sessions now have IDs, actual child-exit tracking,
      cancellable short queries, visible status, and bounded GUI-exit cleanup.
      General session cancellation and native authentication integration remain.

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

- [x] Replace the inference of unauthorized status from missing properties with explicit
      `device`, `offline`, and `unauthorized` states and expose them separately
      from whether the ADB server is running.
- [x] Gate all refresh/reconnect paths on the intentional-stop state from item 01,
      including page remounts. Avoid overlapping slow refreshes.

## General bug review and fixes

### 09. Check for existing bugs and fix confirmed problems

Priority: start before feature implementation and continue with each change.

- [ ] Review the existing source and relevant upstream issues/fixes for bugs in
      startup, session shutdown, device selection, USB/wireless reconnects, saved
      settings, custom binary paths, downloads/updates, and error reporting.
- [ ] Check UI behavior, translations, invalid inputs, timeouts, overlapping
      operations, and process/resource cleanup. Include the supporting fixes in
      items 06–08 rather than duplicating their work.
- [ ] Record each finding with reproduction steps or specific source evidence,
      expected versus actual behavior, affected versions/platforms, and priority.
      Keep suspected bugs separate from confirmed findings.
- [ ] Prioritize crashes, data/settings loss, wrong-device actions, credential
      handling, and broken core workflows before cosmetic issues. Reuse suitable
      upstream fixes with attribution and license compliance.
- [ ] Fix confirmed problems in small, reviewable changes. Add focused regression
      checks where they meaningfully prevent recurrence, then verify the affected
      workflow and record any remaining device/platform testing.
- [ ] Repeat the relevant checks after each new feature to catch regressions in
      existing behavior.

Acceptance: findings are tracked with evidence and disposition; each completed
fix has verification results. Do not mark unresolved or untested behavior fixed.

## Device list and connection presentation

### 10. Review long mDNS names and improve device labels

Priority: alongside device-state work. Owner report: long mDNS names currently
look poor in the app; reproduce the layout issue before choosing the final design.

- [ ] Review actual mDNS service names, their label lengths, and how they appear
      in the device selector/tabs and device details. Distinguish technical DNS
      label/name limits from the space available for readable UI labels.
- [ ] Prefer a concise, recognizable device name in the selector, with a useful
      disambiguator when devices share the same name/model. Keep the complete
      mDNS name available in the connection details, with a way to copy it.
- [ ] Check wrapping, truncation, spacing, and overflow at narrow window widths,
      higher display scaling, and with multiple devices. Use the existing app's
      typography and component styling; make shortened labels accessible beyond
      a mouse-only hover tooltip.
- [ ] Keep display labels separate from the exact ADB transport selector. Never
      shorten or modify the identifier passed to ADB to make a label fit.

Acceptance: long names no longer distort the layout; similar device names remain
distinguishable, and the full connection identifier can still be inspected/copied.

### 11. Investigate duplicate devices and group their connection paths

Priority: alongside item 10. Owner report / possible fix: the same physical device
can appear twice, once as an IP endpoint and once as an mDNS service. Confirm when
this represents two routes to one device before merging entries.

- [ ] Investigate how ADB reports USB, IP, and mDNS transports for the same device,
      including whether an mDNS service resolves to an already listed IP endpoint.
- [ ] Identify individual physical devices using verified identity information,
      then show one device entry with its available connection paths in the device
      information section below the selector. Do not merge devices solely because
      they share a model, friendly name, or an IP address observed at another time.
- [ ] List each route's connection type, complete transport selector/endpoint,
      online/offline/unauthorized state, and which path is currently selected.
      Let the user choose the path used to start a session and disconnect a
      particular wireless route without hiding the device's remaining routes.
- [ ] Preserve device selection as routes appear/disappear or wireless addresses
      change. Represent uncertain identity separately until it can be verified;
      unavailable identity must not cause automatic cross-device grouping.
- [ ] Bind saved device settings and enrolled unlock credentials to verified
      physical identity, then recheck the selected connection before any input.
      Grouping must not weaken item 03's wrong-device protections.

Acceptance: a verified device reachable by IP and mDNS appears once with both
paths in its details; two devices of the same model remain separate. Check route
loss/reconnect, USB plus wireless, unavailable identity, and intentional ADB Stop.

Source starting point for items 10–11:
`application/app/components/devices.vue`, which currently labels device tabs with
the raw ADB selector and requests device properties per selected transport.

## Additional ADB reliability work

### 12. Test multiple ADB processes and servers

- [x] Distinguish transient ADB clients from a listening server, and test multiple
      simultaneous clients sharing one server.
- [x] Test two real servers on separate isolated loopback ports. Verify repeated
      Start reuses an existing server and a second server cannot bind the same
      endpoint. Verify stopping one endpoint leaves the other responsive.
- [x] Isolate test keys, suppress automatic device connections, clean up only
      test-owned processes, and leave the shared default server running.
- [ ] Extend coverage to different ADB versions and explicit custom/remote socket
      configurations, including version-mismatch restart behavior.

The initial real test passed with ADB 37.0.3; see `npm run test:adb` and the
development notes. It does not establish compatibility between different builds.

### 13. Investigate recurring wireless/mDNS discovery loss

Priority: early reliability work. The owner reported a second occurrence on
November 23, 2026. ADB was running with mDNS enabled but listed neither devices nor
mDNS services. A direct connection to the phone's current endpoint worked and
verified the enrolled identity; the server was not restarted.

- [x] Reproduce the loss and distinguish a stopped daemon, an offline/unauthorized
      transport, an obsolete wireless endpoint, and missing mDNS advertisements.
- [ ] Investigate phone advertising, network multicast forwarding, host filtering,
      and ADB discovery using bounded diagnostics. A directed mDNS question from
      UDP port 5353 restored service records while ordinary multicast questions
      received no reply. The latest stable ADB 37.0.3 exhibited the failure.
      The multicast-path root cause is still unconfirmed.
- [x] Add bounded recovery for previously connected local IPv4 devices: learn
      their addresses from online transports, query missing services at most
      every 30 seconds on their directly connected subnet, and let ADB resolve
      the current port/authenticate. Respect intentional Stop. Do not treat an
      address as physical identity, scan arbitrary hosts/ports, or restart ADB.
- [ ] Add clear connection/discovery diagnostics, including first-connection,
      changed-address, IPv6-only, and unreachable-network limitations.
- [ ] Verify repeated disconnect/reconnect and endpoint changes, and ensure any
      recovery respects intentional Stop and other tools sharing the server.

The production recovery passed with a known peer's service missing and the
shared server process unchanged;
repeated GUI outage/cold-start acceptance remains open. Keep personal addresses,
device IDs, pairing material, and raw private logs out of the public repository.

## Settings clarity

### 14. Clearly show when bundled executable defaults are in use

- [ ] Change the SCRCPY and ADB executable fields so an empty custom override
      clearly says **Using bundled default**, rather than suggesting an executable
      is missing. Distinguish a valid default from an unavailable executable.
- [ ] Show the effective executable path and whether it comes from the bundled
      runtime or a custom selection. Keep custom path selection and reset clear.
- [ ] Check a fresh test profile, an existing custom password wrapper, a custom
      ADB executable, and an invalid/missing saved path. Displayed information
      must match the actual executable used by the backend.

### 15. Use an affirmative Check for updates setting

- [ ] Make the setting cover **both ADB and scrcpy updates**, in addition to the
      existing SCRCPY+ app update check. Use the official Google Platform-Tools
      source for ADB and official Genymobile releases for scrcpy, with separate
      installed/available versions and results for each component.
- [ ] Rename **Do not check for updates** to **Check for updates** and invert
      the checkbox/storage logic to match: checked enables checks, unchecked
      disables them. A fresh profile should have the setting checked by default.
- [ ] Preserve the current effective default, which already allows update checks.
      Migrate an existing negative setting by inverting its stored value; someone
      who disabled update checks must keep them disabled after the wording change.
- [ ] Update every read/write path and translation consistently. Verify new-profile
      defaults, both legacy values, checkbox changes, and persistence after restart;
      disabled automatic checks must not issue release-check requests for any
      covered component. Keep checking separate from installation: this checkbox
      must not silently replace binaries or enable automatic installation.

### 16. Keep the viewport steady when toggling Advanced ADB controls

- [ ] Reproduce the screen movement when **Show advanced ADB controls** inserts
      the ADB panel above the current position. Keep the toggle and current reading
      position at the same viewport location when content is added or removed.
- [ ] Consider placing the revealed panel below its toggle or preserving a stable
      visible element as the scroll anchor. Avoid unexpected scrolling, focus
      changes, and layout animations that move the user's place on the page.
- [ ] Check enabling and disabling while scrolled down, keyboard activation,
      narrow windows, and display scaling. Account for the browser's scroll limits
      when hiding content reduces the page height.

Acceptance: toggling the setting leaves the user's visible position and focus
stable wherever the available page height allows it.

## Delivery order and verification

Begin the bug review (09) before feature work; carry its confirmed fixes through
the relevant steps below and repeat affected checks as features are added.

1. ADB status/controls, polling semantics, and typed process/session results
   (01, 06, 08), with device labels and connection grouping (10, 11).
   Include multiple-server tests (12) and investigate recurring discovery loss (13).
2. Independent ADB update staging, activation, rollback, and override preservation
   (02, 07).
3. Native password parity with the existing helper (03).
4. Pattern input prototype and protected-screen detection investigation (04, 05).

Use synthetic credentials and isolated ADB endpoints for automated checks. Keep
device-specific testing explicit. Before changing code, recheck official stable
toolchain/dependency releases and update in-scope pins/lockfiles where compatible.
The initial implementation updates in-scope dependencies and runs isolated ADB
tests; it does not install the fork over the owner's existing application.

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
