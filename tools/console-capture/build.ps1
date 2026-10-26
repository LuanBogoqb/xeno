$ErrorActionPreference = 'Stop'
$tool = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path '.superpowers/toolchains/rust-xbox360'
$actual = & git -C $tool rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne '7b323d1733994add0a89956219ae7b3d830aa7eb') { throw 'Install the pinned toolchain with tools/smc-capture/build.ps1 -InstallToolchain.' }
function Run-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
}
Run-Checked cargo @('+1.97.1','build','--release','--locked','-p','xex-link','--manifest-path',(Join-Path $tool 'Cargo.toml'))
$previousBootstrap = $env:RUSTC_BOOTSTRAP
Push-Location $PSScriptRoot
try {
    Run-Checked cargo @('+1.97.1','test','--lib','--locked')
    $env:RUSTC_BOOTSTRAP = '1'
    Run-Checked cargo @('+1.97.1','build','--release','--locked','--target','powerpc-unknown-xbox360.json',
        '-Zbuild-std=core,compiler_builtins','-Zbuild-std-features=compiler-builtins-mem','-Zjson-target-spec',
        '--config','target.powerpc-unknown-xbox360.rustflags=["-C","link-arg=-Tlink.x","-C","link-arg=--no-rosegment"]')
    New-Item -ItemType Directory -Force dist/Console-Capture | Out-Null
    Run-Checked (Join-Path $tool 'target/release/xex-link.exe') @('target/powerpc-unknown-xbox360/release/console-capture',
        'dist/Console-Capture/default.xex','--manifest','xex.toml')
    Run-Checked python @('verify.py','dist/Console-Capture/default.xex')
    Copy-Item README.md dist/Console-Capture/LEIA-ME.txt
    Copy-Item THIRD-PARTY-LICENSE.txt dist/Console-Capture/THIRD-PARTY-LICENSE.txt
    $hash = (Get-FileHash dist/Console-Capture/default.xex -Algorithm SHA256).Hash.ToLower()
    "$hash  default.xex" | Set-Content dist/Console-Capture/SHA256SUMS.txt -Encoding ascii
    Compress-Archive -Path dist/Console-Capture -DestinationPath dist/Console-Capture.zip -Force
    Write-Output "Ready: $PSScriptRoot/dist/Console-Capture.zip"
} finally {
    $env:RUSTC_BOOTSTRAP = $previousBootstrap
    Pop-Location
}
