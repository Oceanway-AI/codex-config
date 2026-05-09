$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$TauriDir = Join-Path $ScriptDir "tauri-oceanway-config"

Set-Location $TauriDir
& .\build.ps1
