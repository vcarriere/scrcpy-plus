param(
    [string]$ScrcpyDirectory = (Join-Path $env:LOCALAPPDATA 'scrcpy-plus'),
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9-]*$')][string]$PreviewName = 'adb-controls-preview'
)

$ErrorActionPreference = 'Stop'
$project_root = Split-Path $PSScriptRoot -Parent
$destination = Join-Path (Join-Path $project_root 'artifacts') $PreviewName
$tools = Join-Path $destination 'scrcpy'
$configuration = Join-Path $PSScriptRoot 'tauri.preview.conf.json'
$required = @('scrcpy.exe', 'scrcpy-server', 'adb.exe', 'AdbWinApi.dll', 'AdbWinUsbApi.dll', 'LICENSE.txt', 'adb-NOTICE.txt')
foreach ($name in $required) {
    if (!(Test-Path -LiteralPath (Join-Path $ScrcpyDirectory $name) -PathType Leaf)) {
        throw "Required SCRCPY runtime file not found: $name"
    }
}
if ((Test-Path -LiteralPath $destination) -and ((Get-Item -LiteralPath $destination).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'The preview destination must not be a junction or symbolic link.'
}

. (Join-Path $PSScriptRoot 'Use-DevelopmentEnvironment.ps1')
Push-Location (Join-Path $project_root 'application')
try {
    & npm.cmd run tauri -- build --debug --no-bundle --features preview --config $configuration
    if ($LASTEXITCODE -ne 0) { throw 'The test preview build failed.' }
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Path $tools -Force | Out-Null
$binary = Join-Path $project_root 'application\src-tauri\target\debug\scrcpy-plus.exe'
Copy-Item -LiteralPath $binary -Destination (Join-Path $destination 'scrcpy-plus-preview.exe') -Force
$runtime_files = @($required + @(Get-ChildItem -LiteralPath $ScrcpyDirectory -Filter '*.dll' -File | Select-Object -ExpandProperty Name)) | Select-Object -Unique
foreach ($name in $runtime_files) {
    $source = Join-Path $ScrcpyDirectory $name
    $copy = Join-Path $tools $name
    Copy-Item -LiteralPath $source -Destination $copy -Force
    if ((Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $copy).Hash) { throw "Runtime copy verification failed: $name" }
}
Copy-Item -LiteralPath (Join-Path $project_root 'LICENSE') -Destination (Join-Path $destination 'COPYING.scrcpy-plus.txt') -Force
Copy-Item -LiteralPath (Join-Path $project_root 'THIRD_PARTY_NOTICES.md') -Destination $destination -Force

$launcher = @'
@echo off
setlocal
set "WEBVIEW2_USER_DATA_FOLDER="
pushd "%~dp0"
start "" "%~dp0scrcpy-plus-preview.exe"
popd
endlocal
'@
[IO.File]::WriteAllText((Join-Path $destination 'Start Test Build.cmd'), $launcher.Replace("`n", "`r`n"))
$instructions = @'
SCRCPY+ Test Build

Double-click "Start Test Build.cmd". No installer is needed.
The window title is "SCRCPY+ Test Build" and this build uses a separate settings
profile: me.vcarriere.scrcpy-plus.preview. Your installed app and its settings
and password helper are not replaced or imported.

1. Open Settings and enable "Show advanced controls". This reveals ADB management,
   executable locations, and Manage versions together. Language, update checks,
   and Remember SCRCPY config stay in the main section. Saved toggle choices and
   executable overrides are preserved when advanced controls are hidden.
   A fresh profile enables automatic update checks and Remember SCRCPY config,
   with advanced controls hidden. Existing saved choices are preserved.
2. Check the ADB status dot and Running/Stopped text.
3. With mirror sessions closed, click Stop ADB and confirm. This stops the shared
   ADB server and disconnects devices used by other apps too.
4. Return to the main page. It should remain paused and Stopped for at least
   10 seconds. The installed scrcpy-plus app should be closed during this check;
   its existing device polling, or another ADB tool, can restart the shared server.
5. Go back to Settings and click Start ADB. Status should return to Running.
6. If the phone is missing, open Connect Wirelessly and use the phone's current
   Wireless debugging address/port for the first connection. The app remembers
   local IPv4 addresses learned from connected devices. If their mDNS entries
   disappear later, it queries those known addresses for the current service at
   most once every 30 seconds while the Devices page is active. It does not use
   an old debugging port. A changed IP or a first connection can still need the
   current endpoint manually. Intentional Stop also pauses this recovery.
7. In the advanced section's ADB card, compare the selected client's executable/version
   with the running server's executable/version and endpoint. A different server
   is explained; its binary does not prove which app started it. These details
   must not start or replace the server.
8. On the main page, select an online Android 15+ phone and check "Turn off phone
   screen", then "Temporarily turn on phone screen for protected screens".
   Start SCRCPY 5.0.1 using its direct executable, matching scrcpy-server, a local
   ADB server, and a normal main-display session.
   A protected app window or fingerprint prompt should turn the physical panel
   on; leaving it should turn it off after a short stable-clear delay. Protected
   content can still be black in the computer mirror. The actual panel can be
   used for the protected app or fingerprint sensor. Keep this GUI open.
   After each switch, check that the mirror still updates and accepts input.
   Close older test apps and their mirrors before opening this build. An existing
   frozen stream is not replaced by launching a new GUI.
9. Screen switching has separate Pause/Resume controls during its session. Manual
   power changes or phone sleep pause it; Resume explicitly returns control to
   the automatic feature. Closing the mirror ends its monitor. Closing the GUI
   now ends automation and closes this app's owned mirrors, so a mirror cannot
   outlive its privacy monitor. Other apps' mirrors and the shared ADB server
   are not stopped. Unsupported/unreadable state is reported rather than guessed.
10. "Check protected screen" is a read-only snapshot using the same visible
    window/fingerprint detector as automatic switching. It shows progress and
    a checked time. Keep the intended app open on the selected device while
    checking. The separate Pause/Resume and Check buttons wrap with a clear gap.
    Checking does not change power. An undetected signal does not rule out
    protected surfaces or other black-video causes.
11. Device tabs use compact model names. Verified USB/IP/mDNS connections to the
    same physical device share one tab. Select a tab to see its full connection
    names, state, and chosen path. Copy preserves the complete identifier.
    Choose another route with its radio control; Start SCRCPY uses that route.
    Disconnect removes only that wireless path. Unknown identities remain
    separate. Equal-model devices have distinguishing suffixes.
12. The advanced section shows effective SCRCPY/ADB executable paths and whether they
    use a bundled/system default or a custom executable. Missing saved overrides
    explain their fallback; missing selected tools show Unavailable. Reset clears
    only that tool's override. Reading status does not execute either tool.
13. "Automatically check for updates" covers SCRCPY+, Google Platform-Tools/ADB,
    and Genymobile scrcpy, with separate versions, results, and errors.
    "Check for updates" works whether that setting is on or off and leaves
    it unchanged. The button sits alongside the option and wraps below it in
    narrow windows. Its compact height matches the toggle, including larger text.
    It shows progress, current/newer-version results, or an error you can retry.
    A source failure must not hide the other components' results. Checking does
    not install; Advanced has separate automatic-install opt-ins. SCRCPY+ keeps
    its existing upstream release page and does not replace this fork's binary.
    Running ADB markers use the app's purple theme
    in both the main-page version information and the advanced section's ADB card.
14. While partway down Settings, toggle "Show advanced controls" on/off
    using the pointer or Space key. The advanced cards open below the toggle card
    without moving the toggle or focus. Closing it keeps the visible position
    wherever the shorter document's scroll limits allow.
15. Original-code bug fixes: bundled Windows tool paths resolve beside this
    executable even if started from another folder. Version diagnostics do not
    run custom script launchers. Downloads stage/validate before activation,
    preserve selected ADB/custom tools, and report failures without wiping the
    working copy. Close sessions before installing; native installation also
    blocks overlapping commands. Wireless setup validates inputs and ignores
    replies/timers after leaving the page. Saved rotation flags migrate to the
    current scrcpy option. See BUG_REVIEW.md in the source for evidence and limits.
16. Native automatic unlock: select the verified phone and expand "Automatic
    unlock". "Check unlock method" is read-only. Detection distinguishes password,
    PIN, pattern, no secure credential, and unknown; only password playback is
    currently implemented. Password unlock needs Windows, Android 15+ owner user 0,
    a local ADB server, and direct scrcpy 5.0.1 with its matching server. In Advanced
    settings use this preview's bundled/default scrcpy, not the existing password
    wrapper. A PIN/pattern/unknown result must not offer password fallback.
17. Enroll locally in the two masked password fields; do not send a password in
    chat or copy an existing helper credential file. Save does not enable replay.
    After saving, explicitly enable "Automatically unlock this device" and check
    "Turn off phone screen" before Start. Use 4-64 printable ASCII characters.
    Disable recording/capture and custom flags. The password is protected in
    Windows Credential Manager, separate from ordinary SCRCPY settings. It is
    bound to this device/user and persists until replaced or removed. The preview
    does not automatically retrieve/import the existing helper's password.
18. While enabled, startup and relocks during this app's owned session may unlock
    automatically, including deliberate locks. For private entry the phone panel
    is verified Off and the mirror closes; it reopens only after verified unlock
    and Off restoration. Recording, clipboard synchronization, and mirror input
    cannot run through this app during replay. Other ADB clients/physical touch
    are outside its exclusive control. Keep the GUI open; "Stop session" remains
    available during private entry, and enrollment edits are disabled until stop.
    An uncertain/partial attempt blocks further automatic input across restarts.
    Unlock manually once on the same phone/user to recover; do not keep retrying
    a password. Removing the saved password disables enrollment for that device.
19. Native password replay is new source work: read-only Pixel detection,
    non-credential private-editor preparation, and synthetic guards passed.
    The preparation probe read/entered no secret and restored the original phone
    sleep/panel state. Live password parity is still pending; no live native
    password success is claimed by this build. Check mirror responsiveness in a
    scoped test after local enrollment. Fingerprint/face prompts remain on-phone;
    PIN/pattern replay and broader compatibility are deferred under backlog 03.
20. In Advanced, Manage versions now includes independent ADB and SCRCPY choices.
    Verified kept versions remain selectable offline. Fresh ADB downloads must
    have official metadata; old Google URLs are not guessed. Read the linked
    Android SDK agreement before choosing ADB "Download & use"; that deliberate
    action downloads under those terms. Checking availability is not acceptance.
    A downgrade shows current/target versions and requires confirmation; an
    unknown version also requires confirmation. Cancel must not download,
    restore, activate, or change preferences. A successful manual version change
    disables only that tool's automatic installation, preserving automatic checks.
21. Both Advanced automatic-install switches default off. Enabling ADB's switch
    acknowledges the displayed SDK notice. Custom executables remain notify-only.
    Automatic scrcpy changes retain supported 5.0.1 control compatibility.
    Disabling the main automatic-check option prevents background requests and
    automatic tool installation, while manual Check remains read-only and usable.
22. Version activation waits for this app's sessions/ADB work to finish. It never
    stops or restarts a shared server automatically. A client/server protocol
    mismatch requires an explicit Stop before selecting another client. An
    existing valid scrcpy copy is reused; an invalid kept directory is preserved
    for deliberate recovery, rather than overwritten beneath another process.
23. Conditional ADB cache compression is a separate acceptance item. A compressed
    kept version shows "Restoring..." when selected and is validated offline
    before activation, after any downgrade confirmation. Compaction follows a
    successful native selection and saved preferences; cache failure reports a
    warning without undoing the selection. The current client, running server,
    and loaded Windows files must stay expanded; unknown server origins and
    platforms without equivalent in-use-file protection skip compaction. Each
    package must save at least 30% and verify restoration within two seconds.
    "Compress inactive versions" works locally without a release-feed request;
    it handles at most two eligible versions per action and reports remaining work.
    Full SDK/cache acceptance and final integrated results are recorded in
    DEVELOPMENT.md; the existing-preview subset benchmark is not full-package proof.

24. Starting SCRCPY immediately shows a rotating ring and the current startup
    step. Preparation/private entry/verification/video opening should remain
    distinguishable. Manual recovery shows instructions without an idle spinner.
    Hide/reopen progress and navigate to Settings while the session continues.
    The panel closes when the owned child confirms video readiness, not merely
    when its process starts. Quiet/custom output stays explicitly unconfirmed.
    Reduced motion keeps a static indicator. Startup progress does not remove
    password safeguards or establish a faster live-phone unlock time.

25. Password replay removes the previous fixed 80 ms character pause and one
    duplicate preparation snapshot. All per-character/pre-Enter guards remain.
    This is a limited improvement: the costly full diagnostic check still runs
    before each character. Read-only component timings do not prove full unlock
    speed or password replay acceptance. The earlier helper is unchanged.
    Current-user checks use the same Android service directly instead of its
    shell wrapper, preserving the identity/user guards.

If another app starts ADB while this test app is paused, it detects the running
server automatically, resumes device discovery, and enables Stop ADB. No GUI
restart or manual Resume action is needed. The status check does not start or
replace the ADB server itself.

This build tests the ADB controls, known-address discovery recovery, compact
device labels, grouped connection paths, automatic protected-screen /
fingerprint-prompt screen power, and the new password-only native unlock source.
Automatic detection and component-update source work are implemented; real-device
password acceptance remains pending. PIN/pattern replay is still on the backlog.
New update checks and guarded managed-tool selection have public-metadata and
synthetic evidence; final integrated/cache acceptance is recorded separately.
The separate profile starts with default settings and does not automatically use
the existing password wrapper. Close the test app when finished; your installed
app is unchanged and can be opened normally.

Source: https://github.com/vcarriere/scrcpy-plus/tree/codex/adb-controls
Original project: https://github.com/Frontesque/scrcpy-plus (GPL-3.0-only)
scrcpy: https://github.com/Genymobile/scrcpy (Romain Vimont / contributors,
Apache-2.0). See COPYING.scrcpy-plus.txt and scrcpy/LICENSE.txt.
ADB and runtime notices are preserved in scrcpy/adb-NOTICE.txt.
Additional discovery-library notices are in THIRD_PARTY_NOTICES.md.
This is a local test artifact, not a public release or signed installer.
'@
[IO.File]::WriteAllText((Join-Path $destination 'TESTING.txt'), $instructions)
$manifest = [ordered]@{
    source_commit = (& git -C $project_root rev-parse HEAD).Trim()
    source_dirty = (@(& git -C $project_root status --porcelain).Count -gt 0)
    profile = 'me.vcarriere.scrcpy-plus.preview'
    build = 'debug; preview; tauri/custom-protocol'
    files = @(Get-ChildItem -LiteralPath $destination -File -Recurse | Where-Object { $_.Name -ne 'build-manifest.json' } | ForEach-Object {
        [ordered]@{ name = [IO.Path]::GetRelativePath($destination, $_.FullName); sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant() }
    })
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $destination 'build-manifest.json') -Encoding utf8
Write-Output "Portable test build ready: $destination"
