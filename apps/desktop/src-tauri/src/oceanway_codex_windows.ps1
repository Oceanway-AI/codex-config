$ErrorActionPreference = 'Stop'

function Test-CodexBundle($exe) {
    if (!$exe -or ![IO.File]::Exists($exe)) { return $false }
    $name = [IO.Path]::GetFileName($exe)
    $resources = Join-Path ([IO.Path]::GetDirectoryName($exe)) 'resources'
    return [IO.File]::Exists((Join-Path $resources 'app.asar')) -and (
        $name -eq 'Codex.exe' -or ($name -eq 'ChatGPT.exe' -and (
            [IO.File]::Exists((Join-Path $resources 'codex.exe')) -or
            [IO.File]::Exists((Join-Path $resources 'codex-cli\bin\codex.exe')))))
}

function Get-CodexInstances($candidates = (Get-Process -Name 'Codex','ChatGPT','codex-*','chrome_crashpad_handler','node','node_repl' -ErrorAction SilentlyContinue)) {
    $session = [Diagnostics.Process]::GetCurrentProcess().SessionId
    $runtimeRoot = Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\'
    foreach ($p in $candidates) {
        if ($p.SessionId -ne $session) { continue }
        $exe = $p.Path
        if (!$exe) {
            if ($p.ProcessName -in @('codex','ChatGPT') -and !$p.HasExited) { throw "无法确认 $($p.ProcessName)（PID $($p.Id)）的安装位置，请先退出该程序再重试。" }
            continue
        }
        $gui = Test-CodexBundle $exe
        # The desktop and terminal may share an app-server installed outside the app bundle.
        $cli = $p.ProcessName -eq 'codex' -or $p.ProcessName -match '^codex-(x86_64|aarch64)-pc-windows'
        $helper = ($p.ProcessName -like 'codex-*' -and $exe.StartsWith($runtimeRoot, [StringComparison]::OrdinalIgnoreCase)) -or
            ($p.ProcessName -in @('node','node_repl') -and $exe.StartsWith(($runtimeRoot + 'runtimes\'), [StringComparison]::OrdinalIgnoreCase))
        if (!$helper -and !$gui -and !$cli -and ($exe -match '^(.+?)\\resources\\' -or $p.ProcessName -eq 'chrome_crashpad_handler')) {
            $dir = if ($exe -match '^(.+?)\\resources\\') { $Matches[1] } else { [IO.Path]::GetDirectoryName($exe) }
            $helper = (Test-CodexBundle (Join-Path $dir 'Codex.exe')) -or (Test-CodexBundle (Join-Path $dir 'ChatGPT.exe'))
        }
        # A long-lived Windows sandbox service is not a configuration client.
        if (($gui -or $cli -or $helper) -and $p.ProcessName -ne 'codex-windows-sandbox-service') {
            try { $started = $p.StartTime } catch { if ($p.HasExited) { continue }; throw }
            [pscustomobject]@{ Process=$p; Path=$exe; Gui=$gui; Started=$started }
        }
    }
}

function Get-CodexPackages {
    if (Get-Command Get-AppxPackage -ErrorAction SilentlyContinue) {
        Get-AppxPackage -Name '*Codex*' -ErrorAction SilentlyContinue
        Get-AppxPackage -Name '*ChatGPT*' -ErrorAction SilentlyContinue
    }
}

function Get-CodexRegisteredPaths {
    foreach ($hive in 'HKCU:','HKLM:') {
        foreach ($view in 'Software','Software\WOW6432Node') {
            $key = "$hive\$view\Microsoft\Windows\CurrentVersion"
            foreach ($name in 'Codex.exe','ChatGPT.exe') {
                $entry = Get-Item -LiteralPath "$key\App Paths\$name" -ErrorAction SilentlyContinue
                if ($entry) { [Environment]::ExpandEnvironmentVariables(([string]$entry.GetValue('')).Trim('"')) }
            }
            foreach ($entry in (Get-ItemProperty "$key\Uninstall\*" -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -match '^(OpenAI )?(Codex|ChatGPT)(\s|$)' })) {
                if ($entry.DisplayIcon) { [Environment]::ExpandEnvironmentVariables(($entry.DisplayIcon -replace ',\s*-?\d+\s*$','').Trim().Trim('"')) }
                if ($entry.InstallLocation) {
                    foreach ($name in 'Codex.exe','ChatGPT.exe') { Join-Path $entry.InstallLocation $name }
                }
            }
        }
    }
}

function Get-CodexShortcutPaths($folders = @('Programs','CommonPrograms','DesktopDirectory','CommonDesktopDirectory' | ForEach-Object { [Environment]::GetFolderPath($_) })) {
    $shell = New-Object -ComObject WScript.Shell
    try {
        foreach ($dir in $folders) {
            if (!$dir) { continue }
            # Only recurse in the Start menu; the desktop can contain entire workspaces.
            $recurse = $dir -in @([Environment]::GetFolderPath('Programs'),[Environment]::GetFolderPath('CommonPrograms'))
            foreach ($link in (Get-ChildItem -LiteralPath $dir -File -Filter '*.lnk' -Recurse:$recurse -ErrorAction SilentlyContinue | Where-Object { $_.BaseName -match '^(OpenAI )?(Codex|ChatGPT)(\s|$)' })) {
                $shell.CreateShortcut($link.FullName).TargetPath
            }
        }
    } finally { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell) }
}

function Find-CodexTarget($instances, $packages, $previous) {
    $paths = @($instances | Where-Object Gui | Sort-Object @{Expression={$_.Process.MainWindowHandle -ne 0};Descending=$true} | ForEach-Object Path)
    $paths += @($previous)
    $registered = @{}
    foreach ($pkg in $packages) {
        if (!$pkg.InstallLocation) { continue }
        try { $manifest = Get-AppxPackageManifest -Package $pkg.PackageFullName } catch { continue }
        foreach ($app in $manifest.Package.Applications.Application) {
            if (!$app.Executable) { continue }
            $exe = [IO.Path]::GetFullPath((Join-Path $pkg.InstallLocation $app.Executable))
            if (Test-CodexBundle $exe) {
                $registered[$exe] = "$($pkg.PackageFamilyName)!$($app.Id)"
                $paths += $exe
            }
        }
    }
    foreach ($root in @("$env:LOCALAPPDATA\Programs",$env:LOCALAPPDATA,$env:ProgramFiles,$env:ProgramW6432,${env:ProgramFiles(x86)})) {
        if (!$root) { continue }
        foreach ($name in @('Codex','ChatGPT')) {
            $paths += "$root\$name\$name.exe","$root\OpenAI\$name\$name.exe"
        }
    }
    foreach ($exe in $paths) {
        if (Test-CodexBundle $exe) { return [pscustomobject]@{Path=$exe; AppId=$registered[$exe]} }
    }
    foreach ($exe in (Get-CodexRegisteredPaths)) {
        if (Test-CodexBundle $exe) { return [pscustomobject]@{Path=$exe; AppId=$registered[$exe]} }
    }
    foreach ($exe in (Get-CodexShortcutPaths)) {
        if (Test-CodexBundle $exe) { return [pscustomobject]@{Path=$exe; AppId=$registered[$exe]} }
    }
}

function Stop-CodexClients($initial) {
    $known = @{}
    foreach ($row in $initial) {
        $known[$row.Process.Id] = $row.Started
        if (!$row.Process.HasExited -and $row.Gui -and $row.Process.MainWindowHandle -ne 0) {
            try { [void]$row.Process.CloseMainWindow() } catch { if (!$row.Process.HasExited) { throw } }
        }
    }
    $grace = [DateTime]::UtcNow.AddSeconds(4)
    $deadline = $grace.AddSeconds(5)
    $quiet = 0
    do {
        Start-Sleep -Milliseconds 250
        $remaining = @(Get-CodexInstances)
        if (!$remaining.Count) {
            $quiet++
            if ($quiet -ge 3) { return }
            continue
        }
        $quiet = 0
        foreach ($row in $remaining) {
            if ($row.Gui -and !$row.Process.HasExited -and $known[$row.Process.Id] -ne $row.Started) {
                if ($row.Process.MainWindowHandle -ne 0) { throw '检测到新打开的 Codex 窗口，请先保存工作后重试。' }
                # A fresh desktop host may not have created its window yet. Only
                # force a new copy of the GUI binary if it is an Electron helper.
                if ([DateTime]::UtcNow -lt $grace) { continue }
                $procInfo = Get-CimInstance Win32_Process -Filter "ProcessId=$($row.Process.Id)" -ErrorAction SilentlyContinue
                if (!$procInfo -or $procInfo.CommandLine -notmatch '(?:^|\s)--type=(?:renderer|gpu-process|utility|crashpad-handler)(?:\s|$)') { continue }
            }
            if ([DateTime]::UtcNow -lt $grace) { continue }
            # A helper can be respawned by a supervisor. Re-check its identity
            # on every scan and only stop the specific inspected instance.
            try {
                if (!$row.Process.HasExited) {
                    $live = Get-Process -Id $row.Process.Id -ErrorAction SilentlyContinue
                    if ($live -and $live.Path -eq $row.Path -and $live.StartTime -eq $row.Started) { $row.Process.Kill() }
                }
            }
            catch { if (!$row.Process.HasExited) { throw "无法退出 $($row.Process.ProcessName)（PID $($row.Process.Id)），请关闭它后重试。" } }
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Codex 后台仍在运行（PID $($remaining.Process.Id -join ', ')）。请关闭正在使用 Codex 的终端或编辑器，再重试。"
}

function Open-CodexTarget($target) {
    # Use the registered identity. Do not count an already-running broker's PID
    # or a successful ShellExecute call as confirmation that a GUI opened.
    if ($target.AppId) {
        try {
          if (!('OceanwayCodexActivation' -as [type])) {
            $null = Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IOceanwayActivationManager {
    [PreserveSig] int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)] string appId, [MarshalAs(UnmanagedType.LPWStr)] string args, uint options, out uint pid);
}
public static class OceanwayCodexActivation {
    public static uint Activate(string id, string args) {
        var obj = Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")));
        try {
            uint pid;
            Marshal.ThrowExceptionForHR(((IOceanwayActivationManager)obj).ActivateApplication(id, args, 0, out pid));
            return pid;
        } finally { Marshal.ReleaseComObject(obj); }
    }
}
'@
          }
          [void][OceanwayCodexActivation]::Activate($target.AppId, '')
          return
        }
        catch { # Explorer can activate the same entry on systems that reject direct COM activation.
        }
    }
    $shell = New-Object -ComObject Shell.Application
    try {
        if ($target.AppId) { $shell.ShellExecute("shell:AppsFolder\$($target.AppId)", '', '', 'open', 1) }
        else { $shell.ShellExecute($target.Path, '', [IO.Path]::GetDirectoryName($target.Path), 'open', 1) }
    } finally { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell) }
}

function Wait-CodexWindow($target, $seconds=30) {
    $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
    $lastWindows = @()
    do {
        $windows = @(foreach ($row in (Get-CodexInstances)) {
            if ($row.Gui -and $row.Path -eq $target.Path) {
                $hwnd = $row.Process.MainWindowHandle
                if ($hwnd -ne 0) { "$($row.Process.Id):$hwnd" }
            }
        })
        foreach ($window in $windows) { if ($lastWindows -contains $window) { return } }
        $lastWindows = $windows
        Start-Sleep -Milliseconds 300
    } while ([DateTime]::UtcNow -lt $deadline)
    throw '没有检测到 Codex 窗口。配置已保留，请稍等片刻；窗口仍未出现时可再次点击打开，或从开始菜单打开。'
}

function Invoke-CodexLifecycle($action, $previous) {
    if ($action -eq 'discover') {
        $hosts = @(Get-Process -Name 'Codex','ChatGPT' -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq [Diagnostics.Process]::GetCurrentProcess().SessionId -and (Test-CodexBundle $_.Path) } | ForEach-Object { [pscustomobject]@{Gui=$true; Process=$_; Path=$_.Path} })
        return @{found=[bool](Find-CodexTarget $hosts @(Get-CodexPackages) $previous)}
    }
    $instances = @(Get-CodexInstances)
    if ($action -eq 'probe') { return @{running=[bool]$instances.Count} }
    if ($action -eq 'close') {
        $hostPath = $instances | Where-Object Gui | Select-Object -First 1 -ExpandProperty Path
        Stop-CodexClients $instances
        return @{path=$hostPath}
    }
    if ($action -eq 'open' -and $instances.Count) { return @{restartRequired=$true} }
    $target = Find-CodexTarget $instances @(Get-CodexPackages) $previous
    if (!$target) { return @{found=$false} }
    if ($action -eq 'restart' -and $instances.Count) { Stop-CodexClients $instances }
    Open-CodexTarget $target
    Wait-CodexWindow $target
    return @{found=$true}
}
