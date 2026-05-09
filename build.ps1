$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm was not found. Install Node.js, then rerun this script."
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo was not found. Install Rust from https://rustup.rs, then rerun this script."
}

npm install
npm exec tauri -- build --bundles nsis

Write-Host ""
Write-Host "Built Tauri bundles under: $ScriptDir\src-tauri\target\release\bundle"
