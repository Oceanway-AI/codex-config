# Run with Windows PowerShell. Only processes whose executable is in this new temp directory
# are ever passed to the lifecycle functions (even when a real Codex is open).
$ErrorActionPreference = 'Stop'
. ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $PSScriptRoot '../../src-tauri/src/oceanway_codex_windows.ps1'))))
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('ow-codex-lifecycle-' + [guid]::NewGuid().ToString('N'))
$prefix = [IO.Path]::GetFullPath($fixture) + '\'
$identify = ${function:Get-CodexInstances}
$env:LOCALAPPDATA = $fixture
function Get-CodexInstances {
    $rows = @(Get-Process -Name Codex,ChatGPT,node,node_repl -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase) })
    & $identify -candidates $rows
}
function Assert-True($ok, $message) { if (!$ok) { throw $message } }
$exe = Join-Path $fixture 'app\Codex.exe'
$cli = Join-Path $fixture 'bin\codex.exe'
$unrelated = Join-Path $fixture 'chat\ChatGPT.exe'
$privateNode = Join-Path $fixture 'OpenAI\Codex\runtimes\fixture\node_repl.exe'
$unrelatedNode = Join-Path $fixture 'bin\node.exe'
try {
    foreach ($d in @('app\resources','bin','chat\resources','OpenAI\Codex\runtimes\fixture')) { [void][IO.Directory]::CreateDirectory((Join-Path $fixture $d)) }
    foreach ($d in @('app','chat')) { [IO.File]::WriteAllText((Join-Path $fixture "$d\resources\app.asar"),'fixture') }
    $src = @'
using System;
using System.Windows.Forms;
using System.Threading;
using System.Diagnostics;
class CodexLifecycleFixture {
    [STAThread] static void Main(string[] args) {
        if (args.Length == 5 && args[0] == "replace-worker") {
            using (var old = Process.GetProcessById(int.Parse(args[1]))) {
                System.IO.File.WriteAllText(args[3] + ".ready", "ready");
                if (!old.WaitForExit(15000)) return;
            }
            using (var next = Process.Start(new ProcessStartInfo(args[2], args[4] == "gui" ? "" : "background") { UseShellExecute = false })) {
                System.IO.File.WriteAllText(args[3], next.Id.ToString());
            }
            return;
        }
        if (args.Length == 2 && args[0] == "leave-worker") {
            Process.Start(new ProcessStartInfo(args[1], "background") { UseShellExecute = false }); return;
        }
        if (args.Length > 0) { Thread.Sleep(90000); return; }
        Application.Run(new Form { Text = "Oceanway test window" });
    }
}
'@
    Add-Type -TypeDefinition $src -OutputType WindowsApplication -OutputAssembly $exe -ReferencedAssemblies System.Windows.Forms,System.Drawing
    [IO.File]::Copy($exe,$cli)
    [IO.File]::Copy($exe,$unrelated)
    [IO.File]::Copy($exe,$privateNode)
    [IO.File]::Copy($exe,$unrelatedNode)
    $hidden = Start-Process -FilePath $exe -ArgumentList 'background' -WindowStyle Hidden -PassThru
    $worker = Start-Process -FilePath $cli -ArgumentList 'background' -WindowStyle Hidden -PassThru
    $control = Start-Process -FilePath $unrelated -ArgumentList 'background' -WindowStyle Hidden -PassThru
    $ownedNode = Start-Process -FilePath $privateNode -ArgumentList 'background' -WindowStyle Hidden -PassThru
    $normalNode = Start-Process -FilePath $unrelatedNode -ArgumentList 'background' -WindowStyle Hidden -PassThru
    Assert-True ((Invoke-CodexLifecycle probe $null).running) 'Background host was missed'
    Assert-True (@(Get-CodexInstances).Count -eq 3) 'Unexpected process ownership'
    $target = Find-CodexTarget @(Get-CodexInstances) @() $null
    # Match the Store manifest executable, not extension host / command runner AppIds.
    function Get-AppxPackageManifest { @{Package=@{Applications=@{Application=@(
        @{Id='CommandRunner';Executable='bin/codex.exe'},@{Id='App';Executable='app/Codex.exe'}
    )}}} }
    $store = Find-CodexTarget @() @(@{InstallLocation=$fixture;PackageFamilyName='Fixture';PackageFullName='Fixture_1'}) $null
    Assert-True ($store.AppId -eq 'Fixture!App') 'Wrong Start menu entry'
    $old = (Invoke-CodexLifecycle close $null).path
    Assert-True ($old -eq $exe -and $hidden.HasExited -and $worker.HasExited -and $ownedNode.HasExited -and !$normalNode.HasExited -and !$control.HasExited) 'Shutdown failed or affected controls'
    Assert-True (!(Invoke-CodexLifecycle probe $null).running) 'Stale clients remain'
    # Existing install in a custom folder, closed, registered only by a shortcut.
    $wsh = New-Object -ComObject WScript.Shell
    $link = $wsh.CreateShortcut((Join-Path $fixture 'Codex.lnk'))
    $link.TargetPath = $exe
    $link.Save()
    $readShortcuts = ${function:Get-CodexShortcutPaths}
    function Get-CodexRegisteredPaths { $unrelated }
    function Get-CodexShortcutPaths { & $readShortcuts @($fixture) }
    $custom = Find-CodexTarget @() @() $null
    Assert-True ($custom.Path -eq $exe) 'Existing desktop shortcut was not resolved, or unrelated ChatGPT matched'
    $hostExited = Start-Process -FilePath $exe -ArgumentList @('leave-worker',"`"$cli`"") -PassThru -WindowStyle Hidden
    Assert-True ($hostExited.WaitForExit(5000)) 'Parent did not exit'
    $orphans = @(Get-CodexInstances)
    Assert-True ($orphans.Count -eq 1 -and !$orphans[0].Gui -and $orphans[0].Path -eq $cli) 'Orphan scenario was not established'
    $null = Invoke-CodexLifecycle close $null
    Assert-True (!(Invoke-CodexLifecycle probe $null).running) 'Worker survived its parent and configuration preflight'
    # An external supervisor starts a new helper only after the old one was killed.
    $watcher = Join-Path $fixture 'bin\ow-fixture-supervisor.exe'
    [IO.File]::Copy($exe,$watcher)
    foreach ($mode in 'worker','gui') {
        $marker = Join-Path $fixture "$mode.pid"
        $replacement = if ($mode -eq 'gui') { $exe } else { $cli }
        $oldWorker = Start-Process -FilePath $cli -ArgumentList 'background' -WindowStyle Hidden -PassThru
        $supervisor = Start-Process -FilePath $watcher -ArgumentList @('replace-worker',$oldWorker.Id,"`"$replacement`"","`"$marker`"",$mode) -WindowStyle Hidden -PassThru
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        while (![IO.File]::Exists("$marker.ready") -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        Assert-True ([IO.File]::Exists("$marker.ready")) 'Replacement supervisor did not start'
        $reason = $null
        try { $null = Invoke-CodexLifecycle close $null } catch { $reason = $_.Exception.Message }
        Assert-True ($supervisor.WaitForExit(3000) -and [IO.File]::Exists($marker)) 'A replacement was never started'
        $replacementPid = [int][IO.File]::ReadAllText($marker)
        Assert-True ($replacementPid -ne $oldWorker.Id) 'Replacement reused the old PID'
        if ($mode -eq 'worker') {
            Assert-True (!$reason -and !(Get-Process -Id $replacementPid -ErrorAction SilentlyContinue)) "Second-generation worker cleanup failed: $reason"
        } else {
            $newWindow = Get-Process -Id $replacementPid -ErrorAction SilentlyContinue
            Assert-True ($reason -and $newWindow -and $newWindow.MainWindowHandle -ne 0) 'New, unconfirmed user window was closed or preflight reported success'
            $null = Invoke-CodexLifecycle close $null
        }
    }
    # Exercise the actual shell launcher, window check and graceful close.
    Open-CodexTarget $target
    Wait-CodexWindow $target 10
    $before = @(Get-CodexInstances)[0].Process
    function Get-CodexPackages { @() }
    Assert-True ((Invoke-CodexLifecycle open $exe).restartRequired -and !$before.HasExited) 'Open interrupted an existing client without confirmation'
    $opened = Invoke-CodexLifecycle restart $exe
    Assert-True ($opened.found -and $before.HasExited -and @(Get-CodexInstances)[0].Process.Id -ne $before.Id) 'Restart did not create a fresh GUI'
    $null = Invoke-CodexLifecycle close $null
    $failed = $false
    try { Wait-CodexWindow $target 1 } catch { $failed = $true }
    Assert-True $failed 'Reported success without a window'
    # A window seen in only one poll (e.g. a host that crashed on startup) is not success.
    $scan = ${function:Get-CodexInstances}
    $script:firstScan = $true
    function Get-CodexInstances {
        if ($script:firstScan) { $script:firstScan=$false; [pscustomobject]@{Gui=$true; Path=$exe; Process=@{Id=42;MainWindowHandle=123}} }
    }
    try {
        $failed = $false
        try { Wait-CodexWindow $target 1 } catch { $failed = $true }
        Assert-True $failed 'Reported success for a disappearing window'
    } finally { Set-Item Function:Get-CodexInstances -Value $scan }
    Assert-True (!$control.HasExited) 'Control did not survive'
    'PASS: hidden GUI, orphan, respawned worker, newly opened user window preserved, Codex runtime, unrelated Node and ChatGPT, Store entry, visible launch, fresh PID after restart, missing and disappearing window'
} finally {
    if ($supervisor -and !$supervisor.HasExited) { $supervisor.Kill(); [void]$supervisor.WaitForExit(3000) }
    Get-Process -Name Codex,ChatGPT,node,node_repl -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { $_.Kill(); [void]$_.WaitForExit(3000) }
    $resolved = (Resolve-Path -LiteralPath $fixture).Path
    if ($resolved -eq $fixture -and ([IO.Path]::GetDirectoryName($resolved) -eq [IO.Path]::GetTempPath().TrimEnd('\'))) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
