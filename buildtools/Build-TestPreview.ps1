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
   ends automation even if the mirror remains open. Unsupported capabilities or
   unreadable state are reported rather than guessed.
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
13. "Automatically check for updates" controls the existing SCRCPY+ app check.
    "Check for updates" works whether that setting is on or off and leaves
    it unchanged. The button sits alongside the option and wraps below it in
    narrow windows. Its compact height matches the toggle, including larger text.
    It shows progress, current/newer-version results, or an error
    you can retry. Neither check installs updates. Expanded ADB/scrcpy update
    checks remain on the backlog. Running ADB markers use the app's purple theme
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

If another app starts ADB while this test app is paused, it detects the running
server automatically, resumes device discovery, and enables Stop ADB. No GUI
restart or manual Resume action is needed. The status check does not start or
replace the ADB server itself.

This build tests the ADB controls, known-address discovery recovery, compact
device labels, grouped connection paths, and automatic protected-screen /
fingerprint-prompt screen power. Built-in password/pattern unlock and automatic
updates are not implemented yet.
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
