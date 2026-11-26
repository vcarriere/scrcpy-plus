param(
    [Parameter(Mandatory)][string]$AdbExecutable,
    [Parameter(Mandatory)][string]$ScrcpyExecutable,
    [switch]$RunUnlock,
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
$project_root = Split-Path $PSScriptRoot -Parent
$adb_path = (Resolve-Path -LiteralPath $AdbExecutable).Path
$scrcpy_path = (Resolve-Path -LiteralPath $ScrcpyExecutable).Path
foreach ($tool_path in @($adb_path, $scrcpy_path)) {
    if (!(Test-Path -LiteralPath $tool_path -PathType Leaf)) { throw 'Select existing native ADB and scrcpy executables.' }
}
if ($RunUnlock -and !$OutputPath) {
    $OutputPath = Join-Path $project_root ('artifacts/unlock-startup-benchmark-' + [Guid]::NewGuid().ToString('N') + '.json')
}
if ($OutputPath -and (Test-Path -LiteralPath $OutputPath)) { throw 'The benchmark output already exists. Choose a new path to preserve it.' }

. (Join-Path $PSScriptRoot 'Use-DevelopmentEnvironment.ps1')
$environment_keys = @('SCRCPY_PLUS_TEST_ADB', 'SCRCPY_PLUS_TEST_SCRCPY', 'SCRCPY_PLUS_TEST_PASSWORD', 'SCRCPY_PLUS_TEST_VIDEO', 'SCRCPY_PLUS_TEST_FAST_DIAGNOSTICS')
$previous_environment = @{}
foreach ($key in $environment_keys) { $previous_environment[$key] = [Environment]::GetEnvironmentVariable($key, 'Process') }
try {
    [Environment]::SetEnvironmentVariable('SCRCPY_PLUS_TEST_ADB', $adb_path, 'Process')
    [Environment]::SetEnvironmentVariable('SCRCPY_PLUS_TEST_SCRCPY', $scrcpy_path, 'Process')
    $test_name = 'automatic_unlock::tests::live_unlock_benchmark_preflight'
    if ($RunUnlock) {
        Write-Output 'Running one native unlock attempt. Close the current mirror first and leave the Pixel unused until the benchmark finishes. The phone returns to sleep afterward.'
        [Environment]::SetEnvironmentVariable('SCRCPY_PLUS_TEST_PASSWORD', '1', 'Process')
        [Environment]::SetEnvironmentVariable('SCRCPY_PLUS_TEST_VIDEO', '1', 'Process')
        [Environment]::SetEnvironmentVariable('SCRCPY_PLUS_TEST_FAST_DIAGNOSTICS', '1', 'Process')
        $test_name = 'automatic_unlock::tests::live_enrolled_password_startup_unlock'
    }
    $messages = @(& cargo test --manifest-path (Join-Path $project_root 'application/src-tauri/Cargo.toml') --lib $test_name -- --ignored --exact --nocapture 2>&1)
    $exit_code = $LASTEXITCODE
    $messages | ForEach-Object { Write-Output $_ }
    $timing_line = $messages | ForEach-Object { [string]$_ } | Where-Object { $_.StartsWith('Native unlock benchmark:') } | Select-Object -First 1
    if ($RunUnlock -and $timing_line) {
        $result = [ordered]@{
            source_commit = (& git -C $project_root rev-parse HEAD).Trim()
            source_dirty = (@(& git -C $project_root status --porcelain).Count -gt 0)
            scope = 'Native startup request through verified unlock; headless software video; one attempt'
            persistent_diagnostics_required = $true
            success = $exit_code -eq 0
            credential_output = $false
            recording = $false
            phone_sleep_restored = $exit_code -eq 0
        }
        foreach ($name in @('password_entry_started_ms', 'submit_started_ms', 'verified_unlocked_ms', 'headless_video_ready_ms')) {
            $match = [regex]::Match($timing_line, [regex]::Escape($name) + '=Some\((\d+)\)')
            $result[$name] = if ($match.Success) { [long]$match.Groups[1].Value } else { $null }
        }
        if ($null -ne $result.password_entry_started_ms -and $null -ne $result.submit_started_ms) {
            $result.password_entry_stage_ms = $result.submit_started_ms - $result.password_entry_started_ms
        }
        $output_file = [IO.Path]::GetFullPath($OutputPath)
        [void](New-Item -ItemType Directory -Path (Split-Path $output_file -Parent) -Force)
        $result | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $output_file -Encoding utf8
        Write-Output "Timing result saved: $output_file"
    }
    if ($exit_code -ne 0) { throw 'The benchmark did not verify successful completion. Do not retry a blocked password attempt; inspect the native recovery message first.' }
} finally {
    foreach ($key in $environment_keys) { [Environment]::SetEnvironmentVariable($key, $previous_environment[$key], 'Process') }
}
