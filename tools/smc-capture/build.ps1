param([switch]$InstallToolchain)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$tool = Join-Path $repo '.superpowers/toolchains/rust-xbox360'
$revision = '7b323d1733994add0a89956219ae7b3d830aa7eb'

function Run-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed ($LASTEXITCODE)" }
}

if ($InstallToolchain) {
    Run-Checked rustup @('toolchain', 'install', '1.97.1', '--profile', 'minimal', '--component', 'rust-src')
    if (!(Test-Path -LiteralPath $tool)) {
        Run-Checked git @('clone', '--no-checkout', 'https://github.com/landaire/rust-xbox360.git', $tool)
        Run-Checked git @('-C', $tool, 'checkout', '--detach', $revision)
    }
}
if (!(Test-Path -LiteralPath $tool)) { throw 'Toolchain missing. Run build.ps1 -InstallToolchain.' }
$actual = & git -C $tool rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne $revision) { throw 'Unexpected rust-xbox360 revision.' }
Run-Checked cargo @('+1.97.1', 'build', '--release', '--locked', '-p', 'xex-link', '--manifest-path', (Join-Path $tool 'Cargo.toml'))

$previousBootstrap = $env:RUSTC_BOOTSTRAP
Push-Location $PSScriptRoot
try {
    $env:RUSTC_BOOTSTRAP = '1'
    Run-Checked cargo @('+1.97.1', 'test', '--locked', '--lib')
    Run-Checked cargo @('+1.97.1', 'build', '--release', '--locked', '--target', 'powerpc-unknown-xbox360.json',
        '-Zbuild-std=core,compiler_builtins', '-Zbuild-std-features=compiler-builtins-mem', '-Zjson-target-spec',
        '--config', 'target.powerpc-unknown-xbox360.rustflags=["-C","link-arg=-Tlink.x","-C","link-arg=--no-rosegment"]')
    New-Item -ItemType Directory -Force dist/SMC-Capture | Out-Null
    Run-Checked (Join-Path $tool 'target/release/xex-link.exe') @(
        'target/powerpc-unknown-xbox360/release/smc-capture', 'dist/SMC-Capture/default.xex', '--manifest', 'xex.toml')
    Run-Checked python @('verify.py', 'dist/SMC-Capture/default.xex')
    Copy-Item README.md dist/SMC-Capture/LEIA-ME.txt
    Copy-Item THIRD-PARTY-LICENSE.txt dist/SMC-Capture/THIRD-PARTY-LICENSE.txt
    $hash = (Get-FileHash dist/SMC-Capture/default.xex -Algorithm SHA256).Hash.ToLower()
    "$hash  default.xex" | Set-Content dist/SMC-Capture/SHA256SUMS.txt -Encoding ascii
    Compress-Archive -Path dist/SMC-Capture -DestinationPath dist/SMC-Capture.zip -Force
    Write-Output "Ready: $PSScriptRoot/dist/SMC-Capture.zip"
} finally {
    $env:RUSTC_BOOTSTRAP = $previousBootstrap
    Pop-Location
}
