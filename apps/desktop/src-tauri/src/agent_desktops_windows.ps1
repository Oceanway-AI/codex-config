$ErrorActionPreference = 'SilentlyContinue'
$name = $env:OCEANWAY_DESKTOP_NAME
$names = @($name, $name.Replace(' ', ''))
if ($name -eq 'Kimi Code') { $names += 'kimi-code-app' }
if ($name -eq 'Hermes Agent') { $names += 'Hermes' }
$folders = @($names) + $name.ToLower().Replace(' ', '-')
$candidates = @()

function Test-DesktopName($value) {
    foreach ($alias in $names) {
        if ($value -eq $alias -or $value -like "$alias *") { return $true }
    }
    return $false
}

function Get-DesktopIconPath($icon) {
    if (-not $icon) { return }
    # NSIS may only publish DisplayIcon, with an icon index after the path.
    $path = [Environment]::ExpandEnvironmentVariables(($icon -replace ',\s*-?\d+\s*$', '').Trim().Trim('"'))
    if ([IO.Path]::IsPathRooted($path) -and $names -contains [IO.Path]::GetFileNameWithoutExtension($path)) { $path }
}

$local = [Environment]::GetFolderPath('LocalApplicationData')
foreach ($root in @($env:LOCALAPPDATA, $local, (Join-Path $local 'Programs'), (Join-Path $env:LOCALAPPDATA 'Programs'), $env:ProgramFiles)) {
    if (-not $root) { continue }
    foreach ($folder in $folders) {
        foreach ($exe in $names) { $candidates += Join-Path (Join-Path $root $folder) "$exe.exe" }
    }
}
$keys = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')
foreach ($entry in (Get-ItemProperty $keys | Where-Object { Test-DesktopName $_.DisplayName })) {
    $candidates += Get-DesktopIconPath $entry.DisplayIcon
    if ($entry.InstallLocation) {
        foreach ($exe in $names) { $candidates += Join-Path $entry.InstallLocation "$exe.exe" }
    }
}
# Portable and custom-directory installs can be registered only by a shortcut.
$shell = New-Object -ComObject WScript.Shell
try {
    foreach ($root in @([Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('CommonPrograms'))) {
        if (-not $root) { continue }
        foreach ($link in (Get-ChildItem -LiteralPath $root -Recurse -Filter '*.lnk' | Where-Object { Test-DesktopName $_.BaseName })) {
            $shortcut = $shell.CreateShortcut($link.FullName)
            $candidates += Get-DesktopIconPath $shortcut.TargetPath
        }
    }
} finally { if ($shell) { [Runtime.InteropServices.Marshal]::ReleaseComObject($shell) | Out-Null } }
foreach ($pkg in (Get-AppxPackage | Where-Object { $_.Name -like "*$($name.Replace(' ',''))*" -or ($name -eq 'Hermes Agent' -and $_.Name -eq 'NousResearch.HermesBundled') })) {
    foreach ($exe in $names) {
        foreach ($relative in @("app\$exe.exe", "$exe.exe")) { $candidates += Join-Path $pkg.InstallLocation $relative }
    }
}
Get-Process | Where-Object { $names -contains $_.ProcessName } | ForEach-Object { $candidates += $_.Path }
$candidates | Where-Object {
    $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) -and
    (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $_) 'resources\app.asar') -PathType Leaf)
} | Select-Object -Unique -First 1
