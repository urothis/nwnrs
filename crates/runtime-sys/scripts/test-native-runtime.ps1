[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repository = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$buildRoot = if ($env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR
} else {
    Join-Path $repository 'target'
}
$fixtureRoot = Join-Path $buildRoot 'runtime-fixture-windows'
$fixtureHost = Join-Path $fixtureRoot 'nwserver-fixture.exe'
$targets = Join-Path $fixtureRoot 'targets'
$administrationObject = Join-Path $fixtureRoot 'administration.obj'
$runtime = Join-Path $buildRoot 'debug\nwnrs_runtime_sys.dll'
$launcher = Join-Path $buildRoot 'debug\nwnrs.exe'
$output = Join-Path $fixtureRoot 'runtime-output.log'
$consoleOutput = Join-Path $fixtureRoot 'console-output.log'

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

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH = "C:\Program Files\LLVM\bin;$cargoBin;$env:PATH"
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
New-Item -ItemType Directory -Force -Path $fixtureRoot | Out-Null

& cargo build --locked --package nwnrs-runtime-sys --lib
if ($LASTEXITCODE -ne 0) { throw 'runtime DLL build failed' }
& cargo build --locked --package nwnrs
if ($LASTEXITCODE -ne 0) { throw 'launcher build failed' }

& cl.exe /nologo /std:c++17 /EHsc /W4 /WX /MD /c `
    (Join-Path $repository 'crates\runtime-sys\tests\fixtures\administration.cpp') `
    "/Fo:$administrationObject"
if ($LASTEXITCODE -ne 0) { throw 'administration fixture compilation failed' }

& rustc.exe `
    (Join-Path $repository 'crates\runtime-sys\tests\fixtures\host.rs') `
    --edition 2024 `
    -C "link-arg=$administrationObject" `
    -C "link-arg=/DEF:$(Join-Path $repository 'crates\runtime-sys\tests\fixtures\windows-exports.def')" `
    -o $fixtureHost
if ($LASTEXITCODE -ne 0) { throw 'fixture host build failed' }

& cargo run --quiet --locked --package nwnrs-runtime-sys `
    --example write-fixture-target-pack -- $fixtureHost $targets
if ($LASTEXITCODE -ne 0) { throw 'fixture target-pack generation failed' }
if (-not (Test-Path -LiteralPath $runtime)) { throw "runtime DLL is missing: $runtime" }

$env:RUST_LOG = 'warn,nwnrs::launcher=info,nwnrs::runtime=info,nwnrs::script=trace,nwnrs::console=info'
$process = Start-Process `
    -FilePath $launcher `
    -ArgumentList @('run', '--no-tail-logs', '--runtime', $runtime, '--targets', $targets, $fixtureHost) `
    -NoNewWindow `
    -Wait `
    -PassThru `
    -RedirectStandardOutput $consoleOutput `
    -RedirectStandardError $output
if ($process.ExitCode -ne 0) { throw "injected fixture failed with exit code $($process.ExitCode)" }

$process = Start-Process `
    -FilePath $launcher `
    -ArgumentList @('run', '--gui', '--no-tail-logs', '--runtime', $runtime, '--targets', $targets, $fixtureHost) `
    -NoNewWindow `
    -Wait `
    -PassThru `
    -RedirectStandardOutput $consoleOutput `
    -RedirectStandardError $output
if ($process.ExitCode -ne 0) { throw "GUI fixture failed with exit code $($process.ExitCode)" }

foreach ($expected in @(
    'TRACE nwnrs::script: fixture trace message',
    'DEBUG nwnrs::script: fixture debug message',
    ' INFO nwnrs::script: fixture info message',
    ' INFO nwnrs::script: fixture multiline first',
    ' INFO nwnrs::script: fixture multiline second',
    ' INFO nwnrs::script: fixture multiline third',
    ' WARN nwnrs::script: fixture warn message',
    'ERROR nwnrs::script: fixture error message'
)) {
    if (-not (Select-String -LiteralPath $output -SimpleMatch $expected -Quiet)) {
        throw "fixture output is missing: $expected"
    }
}

Write-Output "Windows native runtime fixture passed: $output"
