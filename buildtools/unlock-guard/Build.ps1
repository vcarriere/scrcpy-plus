[CmdletBinding()]
param(
    [switch]$Bootstrap,
    [string]$JavaHome
)

$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$toolRoot = Join-Path $projectRoot '.tools\unlock-guard'
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'toolchain.json') -Raw | ConvertFrom-Json
[IO.Directory]::CreateDirectory($toolRoot) | Out-Null
Add-Type -AssemblyName System.Net.Http
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Get-VerifiedTool($tool, [string]$destination) {
    if (Test-Path -LiteralPath $destination) {
        if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $tool.sha256) {
            throw "Cached tool checksum mismatch: $destination"
        }
        return
    }
    if (-not $Bootstrap) {
        throw 'Run with -Bootstrap once to download the pinned official build-only tools.'
    }
    $temporary = $destination + '.' + [Guid]::NewGuid().ToString('N') + '.part'
    $client = [Net.Http.HttpClient]::new()
    $cancellation = [Threading.CancellationTokenSource]::new([TimeSpan]::FromMinutes(4))
    $response = $null
    $download_stream = $null
    $output = $null
    try {
        $client.Timeout = [TimeSpan]::FromMinutes(4)
        $response = $client.GetAsync($tool.url, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancellation.Token).GetAwaiter().GetResult()
        $response.EnsureSuccessStatusCode() | Out-Null
        if ($response.RequestMessage.RequestUri.Scheme -ne 'https' -or $response.RequestMessage.RequestUri.AbsoluteUri -ne $tool.url) {
            throw 'Unexpected build-tool redirect.'
        }
        if ($response.Content.Headers.ContentLength -gt $tool.maxBytes) {
            throw 'Oversized build-tool download.'
        }
        $download_stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $output = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $buffer = [byte[]]::new(1048576)
        $total = 0L
        while (($read = $download_stream.ReadAsync($buffer, 0, $buffer.Length, $cancellation.Token).GetAwaiter().GetResult()) -ne 0) {
            $total += $read
            if ($total -gt $tool.maxBytes) { throw 'Oversized build-tool download.' }
            $output.Write($buffer, 0, $read)
        }
        $output.Dispose()
        $output = $null
        if ($tool.bytes -and $total -ne $tool.bytes) { throw 'Incomplete build-tool download.' }
        if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $tool.sha256) {
            throw 'Build-tool publisher checksum mismatch.'
        }
        [IO.File]::Move($temporary, $destination)
    } finally {
        if ($output) { $output.Dispose() }
        if ($download_stream) { $download_stream.Dispose() }
        if ($response) { $response.Dispose() }
        $cancellation.Dispose()
        $client.Dispose()
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    }
}

if (-not $JavaHome) {
    $jdkArchive = Join-Path $toolRoot $manifest.jdk.archive
    Get-VerifiedTool $manifest.jdk $jdkArchive
    $JavaHome = Join-Path $toolRoot $manifest.jdk.directory
    if (-not (Test-Path -LiteralPath (Join-Path $JavaHome 'bin\javac.exe'))) {
        # Validate every archive path before extraction into this task's tool root.
        $archive = [IO.Compression.ZipFile]::OpenRead($jdkArchive)
        try {
            $expandedBytes = 0L
            foreach ($entry in $archive.Entries) {
                $relative = $entry.FullName.Replace('/', [IO.Path]::DirectorySeparatorChar)
                $target = [IO.Path]::GetFullPath((Join-Path $toolRoot $relative))
                if (-not $target.StartsWith($JavaHome + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and $target -ne $JavaHome) {
                    throw 'Unsafe JDK archive path.'
                }
                $expandedBytes += $entry.Length
                if ($expandedBytes -gt 1000000000) { throw 'Oversized extracted JDK.' }
            }
        } finally {
            $archive.Dispose()
        }
        [IO.Compression.ZipFile]::ExtractToDirectory($jdkArchive, $toolRoot)
    }
}
$JavaHome = [IO.Path]::GetFullPath($JavaHome)
$java = Join-Path $JavaHome 'bin\java.exe'
$javac = Join-Path $JavaHome 'bin\javac.exe'
$jar = Join-Path $JavaHome 'bin\jar.exe'
$d8 = Join-Path $toolRoot $manifest.d8.archive
Get-VerifiedTool $manifest.d8 $d8

# Isolate each build; no existing app, helper, preference, or phone is touched.
$buildRoot = Join-Path $toolRoot ('build-' + [Guid]::NewGuid().ToString('N'))
$classes = Join-Path $buildRoot 'classes'
$testClasses = Join-Path $buildRoot 'tests'
[IO.Directory]::CreateDirectory($classes) | Out-Null
[IO.Directory]::CreateDirectory($testClasses) | Out-Null
$source = Join-Path $PSScriptRoot 'src\com\scrcpyplus\unlockguard\Main.java'
$sources = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'src') -Recurse -File -Filter '*.java' | Sort-Object FullName)
$sourcePaths = @($sources | ForEach-Object FullName)
& $javac --release $manifest.javaRelease -XDstringConcat=inline -encoding UTF-8 -g:none -d $classes @sourcePaths
if ($LASTEXITCODE -ne 0) { throw 'Helper Java compilation failed.' }
# Android ART rejects Java 11 nestmate accesses without D8 desugaring. Keep
# every shipped class at Java 8, which generates the required accessors.
foreach ($classFile in Get-ChildItem -LiteralPath $classes -Recurse -File -Filter '*.class') {
    $classBytes = [IO.File]::ReadAllBytes($classFile.FullName)
    if ($classBytes.Length -lt 8 -or (([int]$classBytes[6] -shl 8) -bor [int]$classBytes[7]) -ne 52) {
        throw 'Helper classes must use Java 8 bytecode for Android nestmate compatibility.'
    }
}
$testPaths = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'test') -Recurse -File -Filter '*.java' | Sort-Object FullName | ForEach-Object FullName)
& $javac --release $manifest.javaRelease -XDstringConcat=inline -encoding UTF-8 -g:none -cp $classes -d $testClasses @testPaths
if ($LASTEXITCODE -ne 0) { throw 'Helper synthetic-test compilation failed.' }
& $java -cp ($classes + [IO.Path]::PathSeparator + $testClasses) com.scrcpyplus.unlockguard.MainTest
if ($LASTEXITCODE -ne 0) { throw 'Helper synthetic checks failed.' }
$powerCheckCounts = @{}
foreach ($suite in @('PowerProtoTest', 'PowerDumpTest')) {
    $testOutput = (& $java -cp ($classes + [IO.Path]::PathSeparator + $testClasses) ('com.scrcpyplus.unlockguard.' + $suite) | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $testOutput -notmatch '^power-(?:proto|dump) synthetic checks passed: ([0-9]+)$') {
        throw 'Power strict-parser or fallback checks failed.'
    }
    $powerCheckCounts[$suite] = [int]$Matches[1]
    Write-Output $testOutput
}
$classArchive = Join-Path $buildRoot 'classes.jar'
& $jar --create --file $classArchive --no-manifest --date '2026-11-26T00:00:00Z' -C $classes .
if ($LASTEXITCODE -ne 0) { throw 'Helper class packaging failed.' }
$dexArchive = Join-Path $buildRoot 'unlock-guard.jar'
& $java -cp $d8 com.android.tools.r8.D8 --release --min-api $manifest.androidMinApi --no-desugaring --output $dexArchive $classArchive
if ($LASTEXITCODE -ne 0) { throw 'Helper DEX compilation failed.' }
$archive = [IO.Compression.ZipFile]::OpenRead($dexArchive)
try {
    if ($archive.Entries.Count -ne 1 -or $archive.Entries[0].FullName -ne 'classes.dex' -or $archive.Entries[0].Length -gt 262144) {
        throw 'Unexpected helper DEX contents.'
    }
} finally {
    $archive.Dispose()
}
$resource = Join-Path $projectRoot 'application\src-tauri\resources\unlock-guard.jar'
[IO.File]::Copy($dexArchive, $resource, $true)
$receipt = [ordered]@{
    sourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    sources = @($sources | ForEach-Object { [ordered]@{
        path = $_.FullName.Substring($projectRoot.Length + 1).Replace('\', '/')
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    } })
    helperSha256 = (Get-FileHash -LiteralPath $resource -Algorithm SHA256).Hash.ToLowerInvariant()
    helperBytes = (Get-Item -LiteralPath $resource).Length
    javaRelease = $manifest.javaRelease
    androidMinApi = $manifest.androidMinApi
    javaVersion = (& $javac --version | Out-String).Trim()
    d8Version = (& $java -cp $d8 com.android.tools.r8.D8 --version | Out-String).Trim()
    syntheticChecks = 51
    protoSyntheticChecks = $powerCheckCounts['PowerProtoTest']
    powerSyntheticChecks = $powerCheckCounts['PowerDumpTest']
    deviceTested = $false
}
$receipt | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'build-receipt.json') -Encoding UTF8
$receipt | ConvertTo-Json
