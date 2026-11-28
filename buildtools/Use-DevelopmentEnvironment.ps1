$ErrorActionPreference = 'Stop'
$project_root = Split-Path $PSScriptRoot -Parent
$node = Join-Path $project_root '.tools\node-v26.13.1-win-x64'
if (Test-Path -LiteralPath $node) { $env:PATH = $node + ';' + $env:PATH }
$cargo = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path -LiteralPath $cargo) { $env:PATH = $cargo + ';' + $env:PATH }

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio C++ Build Tools are required.' }
$visual_studio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$visual_studio) { throw 'Visual Studio C++ Build Tools were not found.' }
Import-Module (Join-Path $visual_studio 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $visual_studio -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'

# Optional local SDK fallback for this checkout; leave the system installation alone.
$sdk = Join-Path $project_root '.tools\microsoft.windows.sdk.cpp\c'
$libraries = Join-Path $project_root '.tools\microsoft.windows.sdk.cpp.x64\c'
if ((Test-Path -LiteralPath $sdk) -and (Test-Path -LiteralPath $libraries)) {
    $version = Get-ChildItem -LiteralPath (Join-Path $sdk 'Include') -Directory | Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty Name
    $env:INCLUDE = @('ucrt', 'shared', 'um', 'winrt', 'cppwinrt').ForEach({ Join-Path $sdk "Include\$version\$_" }) -join ';'
    $env:INCLUDE += ';' + (Join-Path $env:VCToolsInstallDir 'include')
    $env:LIB = (Join-Path $libraries 'um\x64') + ';' + (Join-Path $libraries 'ucrt\x64') + ';' + (Join-Path $env:VCToolsInstallDir 'lib\x64')
    $env:PATH = (Join-Path $sdk "bin\$version\x64") + ';' + (Join-Path $sdk 'bin\x64') + ';' + $env:PATH
    $env:WindowsSdkDir = $sdk + '\'
    $env:WindowsSDKVersion = $version + '\'
    $env:WindowsSdkBinPath = (Join-Path $sdk 'bin') + '\'
}
