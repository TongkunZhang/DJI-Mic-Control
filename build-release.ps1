#Requires -Version 5
#
# Build DJI Mic Control for Windows release with all path-privacy safeties on.
#
# Produces a single self-contained binary at:
#     Release\windows\DJI Mic Control.exe
# (Tauri's Windows app binary is self-contained bar the system WebView2 runtime,
# so no accompanying directory is needed.) For macOS/Linux, use build-release.sh.
#
# "Safeties on" means every absolute filesystem path Rust would otherwise bake
# into panic/backtrace strings (which `strip` does NOT remove) is remapped away,
# so a crash can never leak the build machine's home directory, username, or
# hostname. The build then re-scans the compiled binary and FAILS if anything
# slipped through.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

# --- Path-privacy safety -----------------------------------------------------
# Strip the home dir and Cargo registry from embedded paths. CARGO_ENCODED_-
# RUSTFLAGS (0x1f-separated) tolerates spaces in paths. We emit both backslash
# and forward-slash prefix variants because rustc's embedded paths can use
# either separator on Windows; the post-build scan is the backstop.
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue  # avoid cargo conflict
$UserHome  = $env:USERPROFILE
$CargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $UserHome '.cargo' }
$UserHomeF  = $UserHome  -replace '\\', '/'
$CargoHomeF = $CargoHome -replace '\\', '/'
$US = [string][char]0x1f
$remaps = @(
    "--remap-path-prefix=$CargoHome\=",
    "--remap-path-prefix=$CargoHomeF/=",
    "--remap-path-prefix=$UserHome\=/",
    "--remap-path-prefix=$UserHomeF/=/"
)
$env:CARGO_ENCODED_RUSTFLAGS = ($remaps -join $US)

foreach ($tool in 'cargo', 'npm') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool not found in PATH."
    }
}

Write-Host '==> Building Windows release (single self-contained exe)'

# --- Build (Tauri runs the frontend `npm run build` itself) ------------------
Push-Location gui
try {
    npm install --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'npm install failed' }
    npx tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally {
    Pop-Location
}

# --- Collect the single exe --------------------------------------------------
$Out = Join-Path $ScriptDir 'Release\windows'
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$Exe = Join-Path $ScriptDir 'target\release\djimic-gui.exe'
if (-not (Test-Path $Exe)) { throw "Expected binary not found: $Exe" }
$Dest = Join-Path $Out 'DJI Mic Control.exe'
Copy-Item $Exe $Dest
Write-Host '    + DJI Mic Control.exe'

# --- Verify no build-machine paths leaked into the binary --------------------
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$text  = [System.Text.Encoding]::GetEncoding(28591).GetString($bytes)  # Latin1
$needles = @("$UserHome\", "$UserHomeF/", "$CargoHome\", "$CargoHomeF/",
             $env:USERNAME, [System.Net.Dns]::GetHostName())
$leaks = $needles | Where-Object { $_ -and $text.Contains($_) } | Select-Object -Unique
if ($leaks) {
    Write-Error ("SECURITY: build-machine paths leaked into the binary: " +
                 ($leaks -join ', '))
    exit 1
}
Write-Host '==> Leak scan clean: no home path, Cargo path, username, or hostname in binary.'

# --- Verify the logo/resources were embedded into the exe --------------------
# tauri-build embeds the Windows resource block (app icon + version info) at
# compile time, so a populated VersionInfo confirms the icon is in the exe.
$vi = (Get-Item $Exe).VersionInfo
if ($vi.ProductName) {
    Write-Host "==> Logo/resources embedded (ProductName: $($vi.ProductName))."
} else {
    Write-Warning 'exe has no embedded version resource - the app icon may be missing.'
}

Write-Host '==> Done. Artifact in Release\windows:'
Get-ChildItem $Out | Select-Object -ExpandProperty Name
