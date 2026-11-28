[CmdletBinding()]
param(
    [ValidateSet(3, 5)]
    [int]$Trials = 5,
    [ValidateSet('Services', 'PowerProto', 'ImeCritical', 'Rehearsal')]
    [string]$Mode = 'Services'
)

$ErrorActionPreference = 'Stop'
if ($Mode -eq 'Rehearsal' -and $PSBoundParameters.ContainsKey('Trials')) {
    throw 'Rehearsal always collects twelve snapshots; do not pass -Trials.'
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$toolRoot = Join-Path $projectRoot '.tools\unlock-guard'
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'toolchain.json') -Raw | ConvertFrom-Json
if ($manifest.javaRelease -ne 8 -or $manifest.androidMinApi -ne 35) {
    throw 'The profile requires the verified Java8/API35 helper compatibility pins.'
}
$normalResource = Join-Path $projectRoot 'application\src-tauri\resources\unlock-guard.jar'
$normalReceipt = Join-Path $PSScriptRoot 'build-receipt.json'
$normalHash = (Get-FileHash -LiteralPath $normalResource -Algorithm SHA256).Hash
$normalReceiptHash = (Get-FileHash -LiteralPath $normalReceipt -Algorithm SHA256).Hash
$jdkArchive = Join-Path $toolRoot $manifest.jdk.archive
$d8 = Join-Path $toolRoot $manifest.d8.archive
if ((Get-FileHash -LiteralPath $jdkArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.jdk.sha256 -or
    (Get-FileHash -LiteralPath $d8 -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.d8.sha256) {
    throw 'The pinned official profile build tools are unavailable or modified.'
}
$javaHome = Join-Path $toolRoot $manifest.jdk.directory
$java = Join-Path $javaHome 'bin\java.exe'
$javac = Join-Path $javaHome 'bin\javac.exe'
$jar = Join-Path $javaHome 'bin\jar.exe'
$javaVersion = (& $javac --version | Out-String).Trim()
$d8Version = (& $java -cp $d8 com.android.tools.r8.D8 --version | Out-String).Trim()
if ($javaVersion -ne ('javac ' + $manifest.jdk.version) -or -not $d8Version.StartsWith('D8 ' + $manifest.d8.version + ' ')) {
    throw 'Profile build tools do not match the pinned versions.'
}

$buildRoot = Join-Path $toolRoot ('profile-' + [Guid]::NewGuid().ToString('N'))
$classes = Join-Path $buildRoot 'classes'
$tests = Join-Path $buildRoot 'tests'
[IO.Directory]::CreateDirectory($classes) | Out-Null
[IO.Directory]::CreateDirectory($tests) | Out-Null
$source = Join-Path $PSScriptRoot 'src\com\scrcpyplus\unlockguard\Main.java'
$sources = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'src') -Recurse -File -Filter '*.java' | Sort-Object FullName)
$normalSourceHashes = @{}
foreach ($normalSource in $sources) { $normalSourceHashes[$normalSource.FullName] = (Get-FileHash -LiteralPath $normalSource.FullName -Algorithm SHA256).Hash }
$profileClass = switch ($Mode) {
    'PowerProto' { 'MainPowerProfile' }
    'ImeCritical' { 'MainImeCriticalProfile' }
    'Rehearsal' { 'MainRehearsalProfile' }
    default { 'MainProfile' }
}
$profile_source = Join-Path $PSScriptRoot ('profile\com\scrcpyplus\unlockguard\' + $profileClass + '.java')
$generatedProfile = Join-Path $buildRoot ($profileClass + '.java')
$profileText = [IO.File]::ReadAllText($profile_source)
if ($Mode -eq 'Rehearsal') {
    if (-not $profileText.Contains('static final int SNAPSHOTS = 12;')) { throw 'Unexpected rehearsal snapshot declaration.' }
} else {
    if (-not $profileText.Contains('static final int TRIALS = 5;')) { throw 'Unexpected profile trial declaration.' }
    $profileText = $profileText.Replace('static final int TRIALS = 5;', ('static final int TRIALS = ' + $Trials + ';'))
}
[IO.File]::WriteAllText($generatedProfile, $profileText, [Text.UTF8Encoding]::new($false))
$compileSources = @($sources | ForEach-Object FullName) + @($generatedProfile)
if ($Mode -in @('ImeCritical', 'Rehearsal')) {
    $compileSources += Join-Path $PSScriptRoot 'profile\com\scrcpyplus\unlockguard\ProfileSupport.java'
}
if ($Mode -eq 'PowerProto') {
    $protoSource = Join-Path $PSScriptRoot 'src\com\scrcpyplus\unlockguard\PowerProto.java'
}
& $javac --release 8 -XDstringConcat=inline -encoding UTF-8 -g:none -d $classes @compileSources
if ($LASTEXITCODE -ne 0) { throw 'Profile Java compilation failed.' }
foreach ($classFile in Get-ChildItem -LiteralPath $classes -Recurse -File -Filter '*.class') {
    $bytes = [IO.File]::ReadAllBytes($classFile.FullName)
    if ($bytes.Length -lt 8 -or (([int]$bytes[6] -shl 8) -bor [int]$bytes[7]) -ne 52) {
        throw 'Profile classes must use Java8 synthetic accessors.'
    }
}
$testSources = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'test') -Recurse -File -Filter '*.java' | Sort-Object FullName | ForEach-Object FullName)
& $javac --release 8 -XDstringConcat=inline -encoding UTF-8 -g:none -cp $classes -d $tests @testSources
if ($LASTEXITCODE -ne 0) { throw 'Profile helper-test compilation failed.' }
& $java -cp ($classes + [IO.Path]::PathSeparator + $tests) com.scrcpyplus.unlockguard.MainTest
if ($LASTEXITCODE -ne 0) { throw 'Profile helper checks failed.' }
$protoChecks = 0
$powerChecks = 0
foreach ($suite in @('PowerProtoTest', 'PowerDumpTest')) {
    $testOutput = (& $java -cp ($classes + [IO.Path]::PathSeparator + $tests) ('com.scrcpyplus.unlockguard.' + $suite) | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $testOutput -notmatch '^power-(?:proto|dump) synthetic checks passed: ([0-9]+)$') {
        throw 'Power proto strict-parser checks failed.'
    }
    if ($suite -eq 'PowerProtoTest') { $protoChecks = [int]$Matches[1] } else { $powerChecks = [int]$Matches[1] }
    Write-Output $testOutput
}
$failureProbe = (& $java -cp $classes ('com.scrcpyplus.unlockguard.' + $profileClass) | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $failureProbe -ne 'PROFILE_UNAVAILABLE') {
    throw 'The unsupported-host profile probe did not fail with its fixed safe token.'
}
$classArchive = Join-Path $buildRoot 'classes.jar'
& $jar --create --file $classArchive --no-manifest --date '2026-11-26T00:00:00Z' -C $classes .
if ($LASTEXITCODE -ne 0) { throw 'Profile Java packaging failed.' }
$artifactRoot = Join-Path $projectRoot 'artifacts'
[IO.Directory]::CreateDirectory($artifactRoot) | Out-Null
$artifactName = switch ($Mode) {
    'PowerProto' { 'unlock-guard-power-proto-profile-' }
    'ImeCritical' { 'unlock-guard-ime-critical-profile-' }
    'Rehearsal' { 'unlock-guard-rehearsal-profile-' }
    default { 'unlock-guard-profile-' }
}
$artifactCount = if ($Mode -eq 'Rehearsal') { 12 } else { $Trials }
$profileJar = Join-Path $artifactRoot ($artifactName + $artifactCount + '.jar')
& $java -cp $d8 com.android.tools.r8.D8 --release --min-api 35 --no-desugaring --output $profileJar $classArchive
if ($LASTEXITCODE -ne 0) { throw 'Profile DEX compilation failed.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($profileJar)
try {
    if ($archive.Entries.Count -ne 1 -or $archive.Entries[0].FullName -ne 'classes.dex' -or $archive.Entries[0].Length -gt 262144) {
        throw 'Unexpected profile archive contents.'
    }
} finally {
    $archive.Dispose()
}
if ((Get-FileHash -LiteralPath $normalResource -Algorithm SHA256).Hash -ne $normalHash -or
    (Get-FileHash -LiteralPath $normalReceipt -Algorithm SHA256).Hash -ne $normalReceiptHash) {
    throw 'The normal helper or receipt changed during the separate profile build.'
}
foreach ($normalSource in $sources) {
    if ((Get-FileHash -LiteralPath $normalSource.FullName -Algorithm SHA256).Hash -ne $normalSourceHashes[$normalSource.FullName]) {
        throw 'Normal helper source changed during the separate profile build.'
    }
}
$receipt = [ordered]@{
    sourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    sources = @($sources | ForEach-Object { [ordered]@{
        path = $_.FullName.Substring($projectRoot.Length + 1).Replace('\', '/')
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    } })
    profileSourceSha256 = (Get-FileHash -LiteralPath $generatedProfile -Algorithm SHA256).Hash.ToLowerInvariant()
    profileSha256 = (Get-FileHash -LiteralPath $profileJar -Algorithm SHA256).Hash.ToLowerInvariant()
    profileBytes = (Get-Item -LiteralPath $profileJar).Length
    profileJar = $profileJar
    trials = if ($Mode -eq 'Rehearsal') { 0 } else { $Trials }
    snapshotCount = if ($Mode -eq 'Rehearsal') { 12 } else { 2 }
    mode = $Mode
    entryPoint = 'com.scrcpyplus.unlockguard.' + $profileClass
    labels = switch ($Mode) {
        'PowerProto' { @('power_text', 'power_text_wakefulness', 'power_proto', 'power_proto_wakefulness') }
        'ImeCritical' { @('ime_normal', 'ime_critical', 'ime_equal') }
        'Rehearsal' { @('SNAPSHOT', 'REHEARSAL_TOTAL_NANOS') }
        default { @('policy', 'displays', 'focus_display', 'input_method', 'lock_settings', 'power', 'SurfaceFlinger') }
    }
    javaRelease = 8
    androidMinApi = 35
    javaVersion = $javaVersion
    d8Version = $d8Version
    syntheticChecks = 51
    protoSyntheticChecks = $protoChecks
    powerSyntheticChecks = $powerChecks
    hostFixedFailureProbe = $true
    normalHelperUnchanged = $true
    normalReceiptUnchanged = $true
    normalSourcesUnchanged = $true
    deviceTested = $false
}
if ($Mode -eq 'PowerProto') {
    $receipt.protoSourceSha256 = (Get-FileHash -LiteralPath $protoSource -Algorithm SHA256).Hash.ToLowerInvariant()
}
if ($Mode -in @('ImeCritical', 'Rehearsal')) {
    $receipt.profileSupportSha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'profile\com\scrcpyplus\unlockguard\ProfileSupport.java') -Algorithm SHA256).Hash.ToLowerInvariant()
    $receipt.maximumResponseBytes = 262144
    $receipt.deadlineMilliseconds = 2500
}
$receipt | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $artifactRoot ($artifactName + $artifactCount + '.json')) -Encoding UTF8
$receipt | ConvertTo-Json
