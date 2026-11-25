# Interactive acceptance on a machine with Codex installed. Use a private profile
# and only stop processes descended from the specific root PID that this test starts.
$ErrorActionPreference = 'Stop'
. ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $PSScriptRoot '../../src-tauri/src/oceanway_codex_windows.ps1'))))
$target = Find-CodexTarget @() @(Get-CodexPackages) $null
if (!$target.AppId) { throw 'No registered Codex desktop found' }
$dir = Join-Path ([IO.Path]::GetTempPath()) ('ow-real-codex-' + [guid]::NewGuid().ToString('N'))
$marker = Join-Path $dir 'ui'
$tracked = @{}
$owned = @{}
$identify = ${function:Get-CodexInstances}

function Update-TestProcesses {
    $rows = @(Get-CimInstance Win32_Process -ErrorAction Stop)
    do {
        $changed = $false
        foreach ($row in $rows) {
            $id = [int]$row.ProcessId
            $parent = $tracked[[int]$row.ParentProcessId]
            if ($tracked.ContainsKey($id) -or !$parent -or !$row.CreationDate -or $row.CreationDate -lt $parent.StartTime -or ($parent.HasExited -and $row.CreationDate -gt $parent.ExitTime)) { continue }
            $child = Get-Process -Id $id -ErrorAction SilentlyContinue
            if ($child -and !$child.HasExited -and $child.StartTime -ge $parent.StartTime) {
                $tracked[$id] = $child
                $changed = $true
            }
        }
    } while ($changed)
}

function Get-CodexInstances {
    Update-TestProcesses
    $alive = @($tracked.Values | Where-Object { !$_.HasExited } | ForEach-Object { $_.Refresh(); $_ })
    & $identify -candidates $alive
}
function Open-CodexTarget($app) {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $app.Path
    $start.Arguments = '--user-data-dir="' + $marker + '"'
    $start.UseShellExecute = $false
    $start.RedirectStandardError = $true
    $start.RedirectStandardOutput = $true
    $start.WorkingDirectory = $dir
    foreach ($key in @($start.EnvironmentVariables.Keys)) {
        if ($key -match '^(CODEX_|OPENAI_|BROWSER_USE_|SKY_|NODE_REPL_|ELECTRON_|ANTHROPIC_)') { $start.EnvironmentVariables.Remove($key) }
    }
    foreach ($pair in @{CODEX_HOME='codex-home';APPDATA='AppData\Roaming';LOCALAPPDATA='AppData\Local'}.GetEnumerator()) {
        $private = Join-Path $dir $pair.Value
        [void][IO.Directory]::CreateDirectory($private)
        $start.EnvironmentVariables[$pair.Key] = $private
    }
    $p = [Diagnostics.Process]::Start($start)
    $p.BeginErrorReadLine()
    $p.BeginOutputReadLine()
    $owned[$p.Id] = $p
    $tracked[$p.Id] = $p
}
try {
    [void][IO.Directory]::CreateDirectory($dir)
    $null = Invoke-CodexLifecycle open $target.Path
    $first = @(Get-CodexInstances | Where-Object { $_.Gui -and $_.Process.MainWindowHandle -ne 0 })[0].Process
    if (!$owned.ContainsKey($first.Id)) { throw 'No independent main window' }
    $null = Invoke-CodexLifecycle restart $target.Path
    $next = @(Get-CodexInstances | Where-Object { $_.Gui -and $_.Process.MainWindowHandle -ne 0 })[0].Process
    if (!$first.HasExited -or $first.Id -eq $next.Id -or !$owned.ContainsKey($next.Id)) { throw 'Restart did not open a new isolated host' }
    "PASS: $($target.AppId) / $([IO.Path]::GetFileName($target.Path)); isolated window PID $($first.Id) -> $($next.Id)"
} finally {
    Update-TestProcesses
    foreach ($p in $tracked.Values) {
        if (!$p.HasExited) { try { $p.Kill(); [void]$p.WaitForExit(3000) } catch { if (!$p.HasExited) { throw } } }
    }
    $resolved = (Resolve-Path -LiteralPath $dir).Path
    if ($resolved -eq $dir -and ([IO.Path]::GetDirectoryName($resolved) -eq [IO.Path]::GetTempPath().TrimEnd('\'))) {
        # Git may have produced read-only pack files in the private CODEX_HOME.
        # Walk only ordinary directories; never descend into a reparse point.
        $stack = New-Object 'System.Collections.Generic.Stack[string]'
        $stack.Push(('\\?\' + $resolved))
        while ($stack.Count -gt 0) {
            foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($stack.Pop())) {
                $attr = [IO.File]::GetAttributes($entry)
                if ($attr -band [IO.FileAttributes]::ReparsePoint) { continue }
                if ($attr -band [IO.FileAttributes]::Directory) { $stack.Push($entry) }
                elseif ($attr -band [IO.FileAttributes]::ReadOnly) { [IO.File]::SetAttributes($entry, $attr -bxor [IO.FileAttributes]::ReadOnly) }
            }
        }
        [IO.Directory]::Delete(('\\?\' + $resolved), $true)
    }
}
