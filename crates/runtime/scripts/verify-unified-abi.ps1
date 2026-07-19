[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string] $UnifiedRoot,

    [Parameter(Mandatory = $true, Position = 1)]
    [string] $Output
)

$ErrorActionPreference = 'Stop'
$repository = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$unified = (Resolve-Path $UnifiedRoot).Path
$unifiedCmake = Join-Path $unified 'CMakeLists.txt'
$unifiedCommitLines = & git -c "safe.directory=$unified" -C $unified rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or -not $unifiedCommitLines) {
    throw "failed to resolve the Unified commit from $unified"
}
$unifiedCommit = ($unifiedCommitLines -join '').Trim()

function Read-CMakeInteger([string] $Name) {
    $pattern = "(?m)^\s*set\($([regex]::Escape($Name))\s+([0-9]+)\)\s*$"
    $matches = [regex]::Matches([IO.File]::ReadAllText($unifiedCmake), $pattern)
    if ($matches.Count -ne 1) {
        throw "could not read one integer $Name from $unifiedCmake"
    }
    return $matches[0].Groups[1].Value
}

$nwnBuild = Read-CMakeInteger 'TARGET_NWN_BUILD'
$nwnRevision = Read-CMakeInteger 'TARGET_NWN_BUILD_REVISION'
$nwnPostfix = Read-CMakeInteger 'TARGET_NWN_BUILD_POSTFIX'

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
    throw 'Visual Studio vswhere.exe is unavailable'
}
$installation = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $installation) {
    throw 'Visual Studio C++ x64 tools are unavailable'
}
$developerShell = Join-Path $installation 'Common7\Tools\VsDevCmd.bat'
$environmentLines = & cmd.exe /d /s /c "`"$developerShell`" -arch=x64 -host_arch=x64 >nul && set"
foreach ($line in $environmentLines) {
    $separator = $line.IndexOf('=')
    if ($separator -gt 0) {
        [Environment]::SetEnvironmentVariable(
            $line.Substring(0, $separator),
            $line.Substring($separator + 1),
            'Process'
        )
    }
}

$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) "nwnrs-abi-probe-$PID"
$probe = Join-Path $temporaryRoot 'abi-probe.exe'
$probeObject = Join-Path $temporaryRoot 'abi-probe.obj'
$outputPath = if ([IO.Path]::IsPathRooted($Output)) {
    [IO.Path]::GetFullPath($Output)
} else {
    [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $Output))
}

try {
    New-Item -ItemType Directory -Force -Path $temporaryRoot | Out-Null
    New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($outputPath)) | Out-Null

    $arguments = @(
        '/nologo',
        '/std:c++17',
        '/EHsc',
        '/W4',
        '/WX',
        "/I$(Join-Path $repository 'crates\runtime\abi\windows-shims')",
        "/external:I$(Join-Path $unified 'NWNXLib')",
        "/external:I$(Join-Path $unified 'NWNXLib\API')",
        "/external:I$(Join-Path $unified 'NWNXLib\External\sqlite3\include')",
        '/external:W0',
        "/DNWNRS_UNIFIED_COMMIT=\`"$unifiedCommit\`"",
        "/DNWNX_TARGET_NWN_BUILD=$nwnBuild",
        "/DNWNX_TARGET_NWN_BUILD_REVISION=$nwnRevision",
        "/DNWNX_TARGET_NWN_BUILD_POSTFIX=$nwnPostfix",
        (Join-Path $repository 'crates\runtime\abi\abi-probe.cpp'),
        "/Fo:$probeObject",
        "/Fe:$probe"
    )
    & cl.exe @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Windows ABI probe compilation failed' }

    $snapshot = & $probe
    if ($LASTEXITCODE -ne 0) { throw 'Windows ABI probe execution failed' }
    $encoding = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText(
        $outputPath,
        (($snapshot -join [Environment]::NewLine) + [Environment]::NewLine),
        $encoding
    )

    & cargo run --quiet --locked --package nwnrs-runtime --example verify-unified-abi -- `
        $outputPath `
        (Join-Path $repository 'crates\runtime\targets')
    if ($LASTEXITCODE -ne 0) { throw 'Windows ABI target-pack verification failed' }
}
finally {
    if (Test-Path -LiteralPath $temporaryRoot) {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
}
