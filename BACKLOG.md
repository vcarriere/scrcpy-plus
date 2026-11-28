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
      for a GUI restart or manual Resume. Native authentication/power monitors
      use the same lifecycle gate.
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

### 02. ADB version selection and automatic updates, independently of scrcpy

Implemented in source; final integrated/cache and real-platform acceptance remain
below. Automatic checks and automatic installation are separate preferences.

- [x] Add stable-version checks through **Check for updates**, separate installed/
      available versions, and Advanced opt-ins to install ADB or scrcpy updates
      automatically when idle. Installation defaults off for each tool; automatic
      checks remain enabled by default. Custom executable selections are notify-only.
- [x] Add an ADB version chooser alongside the existing scrcpy version manager
      in the advanced section. Show installed, selected, and available supported
      Platform-Tools/ADB releases; allow choosing a version independently of
      scrcpy. Verify package availability and authenticity for older versions
      before listing them as installable. Apply the manual-version and downgrade
      behavior in item 07 to both tools.
- [x] Use Google's official SDK Platform-Tools downloads and release metadata.
      Check the package revision/build as well as the ADB protocol version:
      `1.0.41` alone does not identify the Platform-Tools release.
- [x] Stage and validate the matching OS/architecture package before activating
      it. Preserve the complete required runtime set, including both ADB DLLs on
      Windows and applicable notices. Reject unsupported architectures clearly.
- [x] Verify the official download origin and available publisher signatures or
      official integrity metadata. A locally computed hash records received bytes;
      it is not independent proof of authenticity.
- [x] Defer activation while mirroring, unlock attempts, or other affected ADB
      work is active. Do not automatically kill the shared server or replace
      in-use binaries. Keep the previous working version for rollback.
- [x] Preserve pairing/authorization keys, saved connections, custom paths, and
      unlock settings. Treat an externally managed custom ADB as notify-only
      unless the user explicitly chooses managed updates.
- [x] Keep ADB updates separate from scrcpy downloads so selecting a scrcpy
      release does not silently downgrade ADB or replace a custom launcher.
- [x] Reuse verified kept ADB and scrcpy versions offline. Fresh older scrcpy
      selections resolve the publisher's SHA256SUMS when GitHub asset digests are
      absent. List historical ADB downloads only when official metadata verifies
      them; do not guess Google archive addresses. Valid kept scrcpy packages
      are reused, and an invalid retained directory is preserved rather than
      overwritten. Protocol checks include PATH-based defaults before activation.
- [x] Link the Android SDK license beside manual ADB downloads and the Advanced
      ADB automatic-install opt-in. Those deliberate actions acknowledge the
      terms; checking availability alone never records license acceptance.
- [x] Complete integrated native/UI and conditional-cache regressions: 138 native
      and 120 headless UI tests passed; environment/opt-in checks remain distinct.
- [ ] Complete scoped managed-version acceptance for offline checks, interrupted
      downloads, extraction failures, locked/in-use files, unsuccessful activation,
      rollback, and restoration after restart. Owner tools and the shared server
      have not been changed as part of implementation verification.
- [x] Implement conditional compression for kept ADB packages: protect
      the selected client, running server, and loaded Windows files; skip unknown
      server origins and platforms without an equivalent in-use-file guarantee.
      Keep compression only when a package saves at least 30% and its verified
      restore takes at most two seconds. Restore and validate offline before
      activating a compressed version. Compact only after activation and saved
      preferences commit; a cache failure must not undo a successful selection.
      Add a local-only **Compress inactive versions** action, processing at most
      two eligible versions per action and reporting remaining work. Include
      manifest overhead in the savings gate; validate equivalent managed paths.
- [ ] Verify fresh full-SDK compression/restore and real-platform acceptance in
      the preview. The existing-runtime subset benchmark and synthetic cache
      tests are separate evidence; they do not prove every complete SDK package
      will meet the savings/restore thresholds.

Acceptance: update a managed ADB installation without changing scrcpy, credentials,
pairing keys, or active sessions; failed activation retains the working version.
Live public-metadata checks confirmed ADB **37.0.3** and scrcpy **5.0.1**, plus a
selected older scrcpy checksum lookup, without downloading executables or changing
ADB/server/phone state. Discovery remains dynamic. The cache feasibility sample
used only the existing preview's ADB subset: 9,996,465 bytes compressed to 4,572,608
bytes (54.3% saved); three verified restores took 618/669/636 ms. This is not a
full SDK-package benchmark or final cache acceptance.

Source: native `tool_catalog.rs`, `adb_installer.rs`, `downloader.rs`, activation
commands in `lib.rs`, `scrcpy_release.ts`, and item 01's lifecycle code.

### 03. Native automatic unlock, detection, and future unlock methods

Current scope: automatic method detection and **password unlock only**, following
the owner's correction. Item 04 is merged here; PIN and nine-dot pattern replay
remain deferred. The native implementation has passed guarded locked-startup
password tests; broader live acceptance is still pending. A checked item below
does not mean all real-device acceptance checks have passed. The working installed
app and existing password helper are unchanged and are not automatically migrated.

Detection is source-confirmed and now has a read-only Pixel result: Android's
`lock_settings` diagnostics reported `PASSWORD` on SDK 37 for Android user 0.
The phone was safely locked and logically asleep; no credential was retrieved
or entered for that check. AOSP reports `PIN`, `PASSWORD`, `PATTERN`, or `NONE`
per Android user, without exposing the credential. See
[Android lock-settings diagnostics](https://android.googlesource.com/platform/frameworks/base/+/android16-release/services/core/java/com/android/server/locksettings/LockSettingsService.java#3485),
[credential types](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/com/android/internal/widget/LockPatternUtils.java#119),
and [shell diagnostic permission](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/packages/Shell/AndroidManifest.xml#168).
This supports an authorized-ADB, root-free detection path on supported builds;
manufacturer/version compatibility still requires testing. The configured method
and current visible challenge are separate: a password-configured phone can show
a fingerprint prompt, and an app's authentication prompt is not device unlock.

- [x] Add **Automatic unlock** to the selected-device controls with detected
      method, password confirmation/enrollment, replace/remove actions, explicit
      per-device opt-in, session status, and manual-recovery feedback. Enrollment
      starts disabled. Enabling it authorizes startup unlock and subsequent relocks,
      including deliberate locks, only while an owned controllable session runs.
      No global/startup unlock service or permanent idle-prevention setting is added.
- [x] Detect the configured method for the verified selected device/current user
      using bounded, filtered diagnostics. Show Password, PIN, Pattern, No secure
      credential, or Unknown; expose password enrollment/playback only. Missing,
      malformed, duplicate, denied, stale, changed, or mismatched state cannot
      authorize input or silently choose another saved method. Fingerprint/face
      completion stays on the phone. Non-owner users are not supported for replay.
- [x] Store separate metadata/attempt and secret records in Windows Credential
      Manager. Bind both to verified physical identity, Android user, method,
      schema, and random credential generation; protect the password in the
      secret record. Public device identifiers label records, never act as keys.
      Reject invalid/mismatched records and interrupted enrollment generations;
      fail closed with no plaintext fallback on unsupported platforms. Device IP
      or mDNS route alone never authorizes credential use.
- [x] Keep enrollment, decryption, and delivery native; accept 4–64 printable ASCII
      characters, retain no password in ordinary settings/localStorage/profiles,
      and zeroize native secret buffers on release. Deliver text through the
      scrcpy control channel, not shell arguments, interpolation, clipboard, or
      logs. Already-unlocked monitoring reads metadata only and retrieves no secret.
- [x] Require Windows, Android 15+ owner user 0, direct scrcpy 5.0.1 with matching
      server, a local ADB endpoint, and a normal controlled main-display session.
      Require **Turn off phone screen** and restrict launch arguments to supported
      standard options. Reject recording/capture sinks, wrappers, OTG/view-only,
      unrelated/virtual displays, and unsupported custom/control options.
- [x] Independently verify boot readiness, current owner, identity, lock policy,
      SIM/occlusion state, logical wakefulness, display 0 focus, current SystemUI
      password editor/client/input type, and physical panel Off. Ignore cached
      editor history and reject fingerprint panels as password fields. Require
      stable readiness; recheck before each character and before Enter, and
      verify actual awake/unlocked state after submission.
- [x] Close and wait for this session's mirror before retrieving the secret.
      Keep only a control-only channel with no video/audio capture or clipboard
      synchronization; confirm the physical phone panel Off while Android stays
      logically awake. This removes the app's mirror/input/recording paths during
      replay. After credential input, reopen only after verified completion and
      confirmed Off restoration. A proven zero-input abort may reopen a manual
      mirror. Share item 05's power controller and suspend its protected-screen
      switching during replay; a power-restoration failure cannot resubmit a password.
- [x] Persist the attempt block before any password character can be written.
      Partial/uncertain submission remains blocked across restarts, including
      transport loss or unavailable success evidence. Keep observing for verified
      manual unlock of the same device/user before clearing that uncertainty.
      Retry only pre-input transient failures with bounded 5/10/20/30-second
      backoff; power restoration has a separate retry path. Report coarse status
      instead of sensitive diagnostic dumps or repeated credential attempts.
- [x] Serialize attempts with a per-phone lease across application copies and
      keep generation checks throughout replay. Tie automation and control to
      the actual owned child/session. Keep **Stop session** available while the
      mirror is temporarily closed, latch cancellation, disable enrollment changes
      during a session, and close owned mirrors on GUI exit so privacy monitoring
      cannot leave an unmanaged mirror behind. Do not stop unrelated processes,
      mirrors, or the shared ADB server.
- [x] Show startup immediately with a themed rotating ring and the current native
      step: connection checks, preparation, private entry, unlock verification,
      screen restoration, and opening video. Show manual-recovery instructions
      in the same panel without a waiting spinner. Keep progress available across
      navigation; distinguish a running child from confirmed video readiness.
- [x] Investigate password-entry slowness against the earlier helper. Read-only
      native Pixel measurements confirmed about 767 ms for a full snapshot before
      each character. Earlier PowerShell-extracted full-snapshot figures are
      superseded: that harness retained source CRLF and did not validate state.
      Remove the unnecessary 80 ms typing pause
      and one duplicate preparation snapshot while preserving all input guards.
      Use the current-user service directly instead of its shell wrapper.
- [x] Reduce per-character diagnostic overhead without dropping any checks:
      replace ten external constant-marker formatters with builtin echo. The
      compiled native benchmark validated every response; five alternating
      trials measured medians of 766 ms before and 509 ms after (about one-third
      less check time). An anchored concurrent prototype measured 767/770 ms
      and was not included in the application.
- [x] Add a repeatable one-attempt native unlock timing check and run it on the
      explicitly enrolled Pixel. One controlled locked/asleep start verified
      unlock at 13.799 s from the native request; entry began at 9.233 s, submission
      at 12.932 s, and headless video at 16.207 s. The phone returned to sleep;
      no credential content, length, or recording was saved.
- [x] Substantially reduce full startup using a persistent read-only Android
      diagnostic helper, retaining the native identity/user/editor/panel guards,
      per-character checks, cancellation, and failure blocking. Validated Pixel
      snapshots dropped from 626 ms to 95 ms median. Fixed-prefix inventory
      formatting dropped from 425 ms to 244 ms with byte-identical output.
      The final guarded locked-startup run verified unlock at 5.895 s and
      headless video at 7.108 s, versus the baseline 13.799/16.207 s (about 56%
      less complete startup time). Entry/guard time fell from 3.699 s to 0.605 s.
      Independent selected-backend failure never falls back during replay;
      test helpers/JARs were cleaned up and the phone returned to sleep.
- [x] Further reduce native startup without weakening the state checks. Parallel
      private-channel setup, three-property action verification, observed panel
      settling, guarded native wake/tap, and compact exact Power diagnostics
      produced two complete Pixel runs at **4.790/5.337 s** to headless video
      and **3.836/4.223 s** to verified unlock, versus 7.108/5.895 s in v23.
      Keep explicit bouncer preparation; never infer visibility from retained
      editor metadata. Current global display focus and rotation constrain the
      native tap. Power remains in its original query position and validates the
      same wakefulness scalar, with fresh text confirmation only for a valid
      omitted default enum. All per-character guards and failure blocking remain.
- [ ] Broaden end-to-end timing/recovery coverage and compare the complete old
      helper workflow. Preparation was the larger delay in the measured run.
      Diagnostic component timings do not prove all native parity. Keep further optimization
      behind measured results, current identity/user/editor/panel checks,
      cancellation, and failure blocking; whole-string asynchronous input is not
      an atomic replacement for intervening guards.
- [x] Research reusable password/PIN/pattern optimization and measure diagnostic
      locality without input. Three fixed twelve-snapshot Pixel rehearsals took
      **352–386 ms** collecting on the phone versus **459–538 ms** through the
      current persistent host channel. This estimates only **107–152 ms** of
      diagnostic/transport savings for that synthetic workload, not a complete
      unlock gain. Fifteen alternating normal/critical IME pairs returned
      identical filtered state, but each run's median saving was under 1 ms.
      Keep both prototypes test-only; these results do not justify a new
      credential-consuming implementation or a production IME change by themselves.
- [ ] Prioritize preparation/startup measurements over faster typing: separately
      time transport/identity checks, control/helper setup, explicit bouncer
      preparation, stable editor readiness, verification, and video opening.
      Evaluate direct WindowManager Binder calls for the existing dismissal and
      fresh display-size reads before changing their behavior. Retain explicit
      preparation; current editor metadata and keyguard events do not prove a
      visible password field. Keep the proven scrcpy screen-power path.
- [ ] Prototype a shared guarded action engine with separate password, PIN, and
      pattern adapters. Keep device/user/method binding, privacy, one-use attempt
      state, cancellation, enrollment revocation, and final verification common.
      First evaluate phone-side **fresh guard + one action**, with explicit
      display 0 and the verified SystemUI UID. UID targeting narrows the receiver
      but does not bind an exact package/editor/window, may cover shared-UID
      packages, and does not make guard/dispatch atomic.
      Whole-credential phone-local execution additionally requires bounded secret
      lifetime, native durable blocking before transfer, a cancellable one-use
      protocol, disconnect/expiry enforcement, revocation checks, wiping, and
      independent host verification. Keep it separate from the read-only helper
      until deliberately implemented and reviewed. PIN/pattern playback stays off.
- [ ] Evaluate keyguard change notifications as a monitoring trigger to reduce
      the current five-second relock polling delay. Treat events as hints that
      require a fresh full guard; they do not establish bouncer readiness and
      cannot authorize input alone.
- [x] Use one method-aware layout and backend separation for configuration,
      enrollment, privacy, monitoring, and recovery. Detection and the enrolled
      type remain distinct; detecting a different method never converts a saved
      password to a PIN/pattern or authorizes biometric replay. Persistently
      invalidate the enrolled password after an observed known method change;
      require explicit re-enrollment even if Password is subsequently restored.
- [ ] Complete the remaining native parity on the explicitly enrolled Pixel:
      already-unlocked startup, repeated sleeping/startup preparation, active-session
      relock, fingerprint-to-password preparation, independent panel restoration,
      Stop/GUI exit during preparation/input, wireless reconnect/USB replug,
      partial transport failure, persistent block, manual recovery, and cold boot.
      Verify that the reopened mirror stays responsive and credentials never
      appear in output, recordings, process arguments, or app-controlled capture.
- [ ] Expand detection compatibility checks across Android builds/manufacturers,
      multiple users/work profiles, no-secure-credential versus disabled-lock
      states, missing/denied diagnostic services, truncation/format changes, and
      configured-method versus current-prompt mismatches. Do not use a credential
      verification command or attempt merely to discover the unlock method.
- [ ] Offer explicit local helper migration only after native parity is proven.
      The current implementation does not retrieve/import the helper's password;
      enrollment must happen locally in masked app fields. Broader password
      character support and additional OS protected-storage backends remain future
      work, with unsupported combinations shown clearly.
- [ ] Evaluate stronger device authentication if malicious device/host spoofing
      is in scope. Serial/manufacturer/product validation prevents accidental
      wrong-device use but is not cryptographic attestation; pinned device-held
      keys/challenge verification may require a separate companion-app decision.

Deferred methods, integrated from former item 04:

- [ ] Add explicit PIN enrollment/confirmation and a verified numeric entry
      adapter only after password parity. Never reinterpret an existing password
      as a PIN or guess a fallback method. Account for auto-confirm versus an
      explicit submit button, and do not require a password IME editor as PIN
      readiness proof. Prefer verified key events over keypad coordinates where
      supported; stop immediately after independently verified completion.
- [ ] Add **Pattern** as a per-device method using a 3×3 enrollment/confirmation
      grid, reset/remove controls, and the same bound protected storage, privacy,
      attempt serialization, persistent uncertainty block, and manual recovery.
      Hide the traced pattern during replay; require a verified live pattern
      entry surface as well as the configured `PATTERN` type.
- [ ] Prototype one continuous down/move/up gesture through selected dots using
      scrcpy's supported control transport. Separate taps/swipes are insufficient.
      Cover intermediate-dot rules, timing, cancellation, rotation/insets, grid
      positions, and device-display coordinates independent of preview scale/crop.
      Keep one finger/down-time throughout the path; use Cancel to abort rather
      than Up, because Up submits the pattern. Require fresh geometry and a
      method-specific entry-surface proof before starting the stroke.
      Retain attribution/source/license notices for any upstream-derived transport
      changes. Validate synthetic paths before explicitly enrolled live testing;
      distinguish manual click-and-drag from saved automatic pattern playback.

Verification checkpoint: synthetic native/UI checks exercise method parsing,
identity/user/focus/privacy changes, password-only gating, unavailable state,
altered/missing/swapped records, generation changes, attempt ordering, blocked
recovery, session cancellation, and stale frontend responses. The live read-only
Pixel detection and non-credential private-editor preparation passed. The first
preparation probe failed closed because duplicate main/numeric display-group wake
diagnostics were rejected; restricting the query to the main power-state line
resolved that ambiguity. The passing probe verified a stable current display-0
password editor, pinned control, and panel Off, then restored the original
sleep/panel state. It retrieved and entered no secret. Later guarded password
startup tests passed as recorded above. Full helper parity and final feature
acceptance remain open; the November 28 optimization probes did not enter input.
See [DEVELOPMENT.md](DEVELOPMENT.md) for the current evidence and limitations.

Security scope: use established OS protection; files/records are not immutable.
The safeguards do not guarantee secrecy against code running as the same Windows
user, an administrator, or a compromised phone/ADB host. Other authorized ADB
clients and physical touch are outside the app's exclusive input control.
Rollback-resistant attestation is not implied by generation checks. Document
these limits rather than treating public device IDs as secrets or proof of trust.
Do not copy personal IDs, passwords, or private helper logs into this fork.

Source: `automatic_unlock.rs`, `unlock_detection.rs`, `unlock_store.rs`,
`screen_power.rs`, `screen_power_control.rs`, `lib.rs`, and
`application/app/components/automatic_unlock.vue`.

### 04. Pattern unlock — merged into item 03

Number retained for earlier references. The deferred PIN/pattern tasks, shared
method design, privacy guards, and acceptance requirements now live under item 03.
Only automatic detection and password replay are in the current implementation.

### 05. Temporarily turn on the phone screen for protected screens

Implemented for supported Android 15+ main-display sessions with scrcpy 5.0.1,
with live entry/exit tests on the owner's Android 17 phone. Android secure windows (`FLAG_SECURE`),
biometric overlays, logical sleep, and physical panel power are separate signals.

- [x] Start a manual, read-only **Check protected screen** action directly below
      **Turn off phone screen**, using the existing app's styling. Inspect only
      the selected device's visible main-display windows and biometric overlays
      using the automatic controller's detector. Return protected, unflagged, or
      unknown after consistent observations; discard stale-device responses.
      It does not change screen power. Show explicit progress and checked time,
      with independently spaced Pause/Resume and Check buttons.
      The owner selected apps that show black video in scrcpy as the first case.
      The v11 app retry confirmed Authenticator reports as protected and the
      Pause/Check buttons are clearly separated.
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
- [x] Use one session power controller for this feature; native password
      recovery now shares its ownership/lifecycle gate.
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
- [x] Confirm on the owner's normal desktop renderer that the v7 power-control
      change resolves the reported v6 frozen mirror. The owner retried both
      Authenticator/Home and fingerprint-prompt/cancellation and confirmed both
      mirrored-window cases stay responsive. Three full-resolution offscreen
      video On/Off cycles also passed; a panel-only test is insufficient.

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
not remove Android's mirror blocking. The owner also confirmed both v7 desktop
cases stay responsive after retrying the screen switches.

## Supporting fixes found while inspecting upstream

### 06. Preserve process results and own the real session lifecycle

- [x] Replace stdout-only command results with typed success/failure results.
      Report unsuccessful launches, nonzero exits, and short-command timeouts;
      bound captured stdout and avoid accumulating long mirror-session output.
      Keep diagnostics coarse rather than echoing potentially sensitive arguments.
- [x] Add session identifiers, actual child-process ownership, exit notifications,
      and cancellation so authentication/power tasks cannot outlive their session.
      Avoid holding unbounded captured output for long-running sessions.
      Automatic screen sessions now have IDs, actual child-exit tracking,
      cancellable short queries, visible status, and bounded GUI-exit cleanup.
      General session ownership/cancellation and native authentication are now
      integrated in source; GUI exit closes owned mirrors. Real-device native
      unlock parity and interrupted-input acceptance remain under item 03.

This is a prerequisite for dependable native unlock and useful ADB error status.

### 07. Make updates preserve working tools, overrides, and chosen versions

- [x] `install_scrcpy_release` previously overwrote both saved binary paths.
      Preserve explicit custom tools and native unlock preferences; make changes
      deliberate and reversible.
- [x] `download_and_extract` previously removed an existing version directory
      before extraction. Stage and validate first, then activate; an extraction
      failure must not destroy the working copy.

The original-code review fixed these two inherited behaviors with isolated
regressions; see [BUG_REVIEW.md](BUG_REVIEW.md). The version-choice safeguards
below are now implemented in source; final integrated acceptance remains open.

- [x] After a successful manual ADB or scrcpy version change, automatically
      disable that tool's automatic installation so the selected version is
      retained. Persist and clearly show that choice; re-enabling installation must be
      deliberate. Cancelled or failed changes leave the previous update setting
      intact. Keep automatic checking distinct from automatic installation, and
      retain the explicit manual check from item 15.
- [x] Before activating an older ADB or scrcpy version, show an explicit
      **Downgrade ADB?** or **Downgrade scrcpy?** warning with the current and
      requested versions. Explain potential compatibility/security regressions
      and that this tool's automatic installation will be disabled after the change. Require
      confirmation; cancellation leaves executables and preferences unchanged.
      Compare actual release/build versions rather than ADB's protocol version.
      An unknown current/target version requires a separate confirmation rather
      than being interpreted as zero. Cancelled prompts perform no download,
      restore, path change, or update-preference change.
- [ ] Verify upgrades, downgrades, unchanged-version selections, warning
      confirmation/cancellation, failed activation, saved update preferences
      after restart, and manual checks while automatic updates are disabled.

### 08. Keep device state separate from daemon state

- [x] Replace the inference of unauthorized status from missing properties with explicit
      `device`, `offline`, and `unauthorized` states and expose them separately
      from whether the ADB server is running.
- [x] Gate all refresh/reconnect paths on the intentional-stop state from item 01,
      including page remounts. Avoid overlapping slow refreshes.

## General bug review and fixes

### 09. Review the original code and fix confirmed problems

Scope: the original upstream code at `30ee624b37d28c79af03517b713f95026d388266`,
compared with the current fork. The separate final-project review is item 18.

- [x] Review the existing source and relevant upstream issues/fixes for bugs in
      startup, session shutdown, device selection, USB/wireless reconnects, saved
      settings, custom binary paths, downloads/updates, and error reporting.
- [x] Check UI behavior, translations, invalid inputs, timeouts, overlapping
      operations, and process/resource cleanup. Include the supporting fixes in
      items 06–08 rather than duplicating their work.
- [x] Record each finding with reproduction steps or specific source evidence,
      expected versus actual behavior, affected versions/platforms, and priority.
      Keep suspected bugs separate from confirmed findings.
- [x] Prioritize crashes, data/settings loss, wrong-device actions, credential
      handling, and broken core workflows before cosmetic issues. Reuse suitable
      upstream fixes with attribution and license compliance.
- [x] Fix confirmed problems in small, reviewable changes. Add focused regression
      checks where they meaningfully prevent recurrence, then verify the affected
      workflow and record any remaining device/platform testing.
- [x] Run the relevant regression checks for this original-code pass. Continue
      affected checks during feature work; reserve the final integrated pass for 18.

Acceptance: findings are tracked with evidence and disposition; each completed
fix has verification results. Do not mark unresolved or untested behavior fixed.

Original-code review completed. [BUG_REVIEW.md](BUG_REVIEW.md) records the fixed
defects, related upstream issues, checks, and the remaining low-priority/product
and cross-platform follow-ups. This is a scoped review, not a bug-free guarantee.

## Device list and connection presentation

### 10. Review long mDNS names and improve device labels

Implemented alongside item 11. A reproduced narrow-window case showed the old
long tab text painting over the details below; the revised labels keep the
original tab and card styling.

- [x] Review actual mDNS service names, their label lengths, and how they appear
      in the device selector/tabs and device details. Distinguish technical DNS
      label/name limits from the space available for readable UI labels.
- [x] Prefer a concise, recognizable device name in the selector, with a useful
      disambiguator when devices share the same name/model. Keep the complete
      mDNS name available in the connection details, with a way to copy it.
- [x] Check wrapping, truncation, spacing, and overflow at narrow window widths,
      higher display scaling, and with multiple devices. Use the existing app's
      typography and component styling; make shortened labels accessible beyond
      a mouse-only hover tooltip.
- [x] Keep display labels separate from the exact ADB transport selector. Never
      shorten or modify the identifier passed to ADB to make a label fit.

Acceptance: long names no longer distort the layout; similar device names remain
distinguishable, and the full connection identifier can still be inspected/copied.

### 11. Investigate duplicate devices and group their connection paths

Implemented alongside item 10. A real IP connection added beside the phone's
existing mDNS connection was grouped into one verified physical device; checked
disconnect removed only the test-owned IP route and retained the original path.

- [x] Investigate how ADB reports USB, IP, and mDNS transports for the same device,
      including whether an mDNS service resolves to an already listed IP endpoint.
- [x] Identify individual physical devices using verified identity information,
      then show one device entry with its available connection paths in the device
      information section below the selector. Do not merge devices solely because
      they share a model, friendly name, or an IP address observed at another time.
- [x] List each route's connection type, complete transport selector/endpoint,
      online/offline/unauthorized state, and which path is currently selected.
      Let the user choose the path used to start a session and disconnect a
      particular wireless route without hiding the device's remaining routes.
- [x] Preserve device selection as routes appear/disappear or wireless addresses
      change. Represent uncertain identity separately until it can be verified;
      unavailable identity must not cause automatic cross-device grouping.
- [x] Pass the verified physical identity and exact connection to session startup;
      recheck the current transport, identity, selected ADB, and server before
      launch or a checked wireless disconnect. Reject device-selector overrides.
      Existing Remember SCRCPY config remains global. Future per-device settings
      and native credential enrollment must use this identity and still recheck
      before each input under item 03; no credentials are migrated here.

Acceptance: a verified device reachable by IP and mDNS appears once with both
paths in its details; two devices of the same model remain separate. Check route
loss/reconnect, USB plus wireless, unavailable identity, and intentional ADB Stop.

Source starting point for items 10–11:
`application/app/components/devices.vue` and native `device_inventory.rs`.
Synthetic checks cover USB plus wireless, equal models, unavailable identities,
route loss/replacement, intentional Stop, keyboard use, and larger text/display
scaling. Physical USB combination testing remains additional hardware coverage;
the live grouping/disconnect check used IP plus mDNS.

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

Implemented defaults: automatic update checks and Remember SCRCPY config are on;
Show advanced controls is off. Existing saved choices are preserved. The compact
manual **Check for updates** button sits alongside its automatic-check option.

### 14. Clearly show when bundled executable defaults are in use

- [x] Change the SCRCPY and ADB executable fields so an empty custom override
      clearly says **Using bundled default**, rather than suggesting an executable
      is missing. Distinguish a valid default from an unavailable executable.
- [x] Show the effective executable path and whether it comes from the bundled
      runtime or a custom selection. Keep custom path selection and reset clear.
- [x] Check a fresh test profile, an existing custom password wrapper, a custom
      ADB executable, and an invalid/missing saved path. Displayed information
      must match the actual executable used by the backend.

The read-only native status resolves effective paths without running either tool
or a wrapper. Missing saved overrides explain their fallback; a selected custom
file deleted afterward stays explicitly custom/unavailable. Reset is per tool.

### 15. Automatic and manual update checks

The checker now includes SCRCPY+, ADB, and scrcpy. SCRCPY+ retains its existing
upstream release feed and download link; it does not automatically replace this
fork's application binary. Tool installation has separate Advanced opt-ins.

The agreed labels are **Automatically check for updates** for the toggle and
**Check for updates** for the manual button.

- [x] Make the setting cover **both ADB and scrcpy updates**, in addition to the
      existing SCRCPY+ app update check. Use the official Google Platform-Tools
      source for ADB and official Genymobile releases for scrcpy, with separate
      installed/available versions and results for each component.
- [x] Rename the current toggle to **Automatically check for updates** so its
      scope is clear. It controls background checks, not automatic installation.
      Preserve the current preference and update all translations. Avoid
      **Automatic updates**, which could imply that updates install themselves.
- [x] Add a **Check for updates** button that works whether automatic checks
      are on or off, without changing that saved preference. Use the existing
      SCRCPY+ release source and show progress, available/current results, or an
      error with retry. Share pending checks across pages and disable duplicate
      requests. Manual checking does not install updates or replace executables.
- [x] Extend the manual button to ADB and scrcpy with the expanded checker;
      show separate installed/available versions, results, and errors per component.
- [x] Rename **Do not check for updates** to **Check for updates** and invert
      the checkbox/storage logic to match: checked enables checks, unchecked
      disables them. A fresh profile should have the setting checked by default.
- [x] Preserve the current effective default, which already allows update checks.
      Invert the checkbox's read/write mapping to the existing negative key so
      existing choices and older builds remain compatible; someone who disabled
      update checks keeps them disabled after the wording change.
- [x] Update the current setting and translations consistently. Verify new-profile
      defaults, both legacy values, checkbox changes, and persistence after restart;
      disabled automatic checks issue no SCRCPY+ release request. Keep checking
      separate from installation: this checkbox does not install or replace tools.
- [x] Apply the automatic-check preference to all covered components. Disabling
      it prevents background release requests for SCRCPY+, ADB, and scrcpy, while explicit
      manual checks remain available and leave the saved preference unchanged.
      Each component reports its own version/result/error so an unavailable
      source does not hide successful checks for the other components. Shared
      pending state prevents duplicate checks. Manual checks do not install.
- [ ] Record the final integrated native/UI acceptance results, including partial
      source failures, disabled automatic checks, explicit manual checks, idle
      installation, custom paths, downgrade cancellation, and restart persistence.

### 16. Keep the viewport steady when toggling advanced controls

- [x] Reproduce the screen movement when **Show advanced controls** inserts
      the ADB panel above the current position. Keep the toggle and current reading
      position at the same viewport location when content is added or removed.
- [x] Consider placing the revealed panel below its toggle or preserving a stable
      visible element as the scroll anchor. Avoid unexpected scrolling, focus
      changes, and layout animations that move the user's place on the page.
- [x] Check enabling and disabling while scrolled down, keyboard activation,
      narrow windows, and display scaling. Account for the browser's scroll limits
      when hiding content reduces the page height.

Acceptance: toggling the setting leaves the user's visible position and focus
stable wherever the available page height allows it.

The panel now appears immediately after the settings-toggle card. The original
pointer case moved the toggle by 580 pixels; pointer, narrow-keyboard, and
larger-text checks now preserve its position exactly. Focus remains on the
toggle. A separate collapse check covers the shorter document's scroll limit.
The shared advanced section now also contains executable locations and version
management, with the same stable-position behavior and saved visibility choice.

### 17. Review and reduce button and field heights

- [ ] Review the current button and input-field heights, using **Connect
      Wirelessly** as the owner's preferred sizing reference. Include the SCRCPY
      and ADB executable path fields and their Browse/Reset buttons.
- [ ] Reduce excessive height and vertical padding where appropriate. Keep
      related buttons and fields aligned, preserve the existing theme and
      typography, and retain the compact update button's alignment with its toggle.
- [ ] Check text/icon clipping, focus indicators, keyboard operation, narrow
      windows, larger text, and display scaling. Keep controls readable and easy
      to use without overlaps.

Acceptance: buttons and fields feel consistently compact, guided by the Connect
Wirelessly button's proportions, with readable contents and aligned controls.

### 18. Final-project bug review and regression fixes

Run after the planned features and fixes are integrated; keep this separate from
the original-code review in item 09.

- [ ] Review the complete final fork, including interactions between original
      code, new features, upgrades, settings migration, and error handling.
- [ ] Repeat startup/shutdown, session ownership, device selection, USB/wireless
      reconnects, mDNS recovery, protected-screen switching, native unlock, version
      management, downloads/rollback, and saved/custom configuration checks for
      the features actually shipped.
- [ ] Check translations, keyboard use, narrow windows, display scaling, invalid
      inputs, overlapping operations, and cleanup. Cover supported platforms and
      keep synthetic checks distinct from actual device acceptance.
- [ ] Revisit unresolved findings from item 09; fix confirmed regressions, run
      affected checks, and record remaining compatibility limits before release.
- [ ] Complete Italian/Japanese translation coverage and decide whether to wire
      or remove the original inactive Discord placeholder once a current official
      destination is known. Include package-install crash recovery, cross-process
      locking, decompressed-size limits, and real-release/platform acceptance.

### 19. SCRCPY+ mirror window with status and privacy overlays

Backlog only; implementation is deferred at the owner's request. Initial source
and documentation review completed November 26, 2026. No wrapper prototype or
performance benchmark has been run.

- [x] Check initial feasibility. Windows supports parenting native windows, SDL3
      can render into an existing native window, and scrcpy already handles video
      orientation/aspect changes. This supports a Windows-first investigation;
      it does not establish a working embedded scrcpy/Tauri combination.
- [ ] Keep a SCRCPY+ outer mirror window present while its owned scrcpy display
      starts, stops, reconnects, or closes for private password entry. Reuse the
      native startup/unlock progress phases for overlays and manual guidance.
      Keep mirror visibility, actual capture/playback, and physical phone-screen
      power as distinct controls/states.
- [ ] Design live, hidden/black, and optional dimmed/frozen-background states.
      Prefer black during credential entry. Any frozen background must precede
      private entry and stay transient; never capture or retain password frames.
      An overlay is not a privacy boundary: retain verified capture shutdown and
      input gating, and reveal the mirror only after safe state and fresh video
      are confirmed. Preserve protected-screen fallback behavior.
- [ ] Investigate hosting the existing owned native renderer first, avoiding
      screenshot polling, another encoder, or passing every frame through JS.
      Compare native child-window hosting with an SDL host integration before
      committing to an architecture. The current Wry 0.57.0 Windows backend uses
      windowed WebView2; HTML overlays over a native child cannot be assumed to
      work correctly. Resolve native layering/composition, focus, input queues,
      DPI compatibility, close/recreate ownership, and unsupported custom tools.
- [ ] Preserve portrait/landscape and 180/270-degree changes, aspect fitting,
      resizing/fullscreen, multi-monitor DPI, keyboard shortcuts, and accurate
      mouse/touch coordinates. Block input during unsafe transitions. Keep the
      outer window stable across inner-window recreation and reconnects.
- [ ] Benchmark against standalone scrcpy on the same phone, codec, resolution,
      frame rate, and connection: startup/first frame, FPS/drops, input latency,
      CPU/GPU load, and memory, including repeated rotation and overlay/private
      entry transitions. Native hosting should avoid a second decode/re-encode
      path (architectural expectation, not a measured performance claim).
      Define an acceptable regression budget from those results and preserve a
      standalone fallback. Assess macOS/Linux separately; do not assume Win32
      parenting applies to Cocoa or Wayland.

Complexity: a Windows proof of concept is plausible; a robust integrated wrapper
is substantial work because of composition, focus, scaling, privacy, and recovery.
Rotation support is available upstream, but integration still needs testing.

References: [Windows SetParent and DPI constraints](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setparent),
[SDL3 native-window properties](https://wiki.libsdl.org/SDL3/SDL_CreateWindowWithProperties),
[scrcpy 5.0.1 video/orientation](https://github.com/Genymobile/scrcpy/blob/v5.0.1/doc/video.md),
[scrcpy screen implementation](https://github.com/Genymobile/scrcpy/blob/v5.0.1/app/src/screen.c),
and [WebView2 hosting/composition/input tradeoffs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting).

## Delivery order and verification

Begin the bug review (09) before feature work; carry its confirmed fixes through
the relevant steps below and repeat affected checks as features are added.

1. ADB status/controls, polling semantics, and typed process/session results
   (01, 06, 08), with device labels and connection grouping (10, 11).
   Include multiple-server tests (12) and investigate recurring discovery loss (13).
2. Independent ADB update staging, activation, rollback, and override preservation
   (02, 07).
3. Native automatic detection and password parity with the existing helper (03).
4. After password acceptance, deferred PIN/pattern adapters within item 03;
   continue protected-screen compatibility investigation under item 05.

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
- [Microsoft DPAPI current-user encryption and integrity protection](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata).
- [scrcpy mouse input modes](https://github.com/Genymobile/scrcpy/blob/master/doc/mouse.md).

Recheck the licenses and distribution terms for any tools or third-party code
actually incorporated in later work, and retain their required notices.
