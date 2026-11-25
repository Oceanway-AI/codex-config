param([switch]$VerifyOnly, [switch]$Elevated)
$ErrorActionPreference = 'Stop'
# PowerShell 7 hosts can pass an incompatible PSModulePath to Windows PowerShell.
# Resolve the system modules from this process's own runtime first.
$env:PSModulePath = (Join-Path $PSHOME 'Modules') + ';' + [Environment]::GetEnvironmentVariable('PSModulePath', 'Machine')
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$transcribing = $false
if ($env:OCEANWAY_INSTALL_LOG) {
    Start-Transcript -LiteralPath $env:OCEANWAY_INSTALL_LOG -Force | Out-Null
    $transcribing = $true
}

try {
    $manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'package.json') -Raw | ConvertFrom-Json
    foreach ($item in $manifest.files) {
        Write-Output "Checking file: $($item.name)"
        $file = Join-Path $PSScriptRoot $item.name
        if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing file: $($item.name). Extract the entire ZIP first." }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $item.sha256) {
            throw "File verification failed: $($item.name). Download the package again."
        }
    }
    $package = Join-Path $PSScriptRoot $manifest.package
    Write-Output 'Checking publisher signature...'
    $signature = Get-AuthenticodeSignature -LiteralPath $package
    # Windows package deployment validates the publisher's signature as well.
    if ($signature.Status -ne 'Valid') { throw "Publisher signature verification failed: $($signature.StatusMessage)" }
    if ($VerifyOnly) { Write-Output "Verified: $($manifest.name) $($manifest.version)"; exit 0 }
    if ([Environment]::OSVersion.Version.Build -lt 19041) { throw 'Windows 10 version 2004 or newer is required.' }
    if (![Environment]::Is64BitOperatingSystem) { throw 'A 64-bit Windows installation is required.' }

    $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (!$admin) {
        if ($Elevated) { throw 'Administrator approval is required to install the desktop application.' }
        $script = $PSCommandPath.Replace("'", "''")
        $command = "& '$script' -Elevated; exit `$LASTEXITCODE"
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
        $process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded)
        if ($process.ExitCode -ne 0 -and (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'install-error.txt'))) {
            Write-Host (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'install-error.txt') -Raw) -ForegroundColor Red
        }
        exit $process.ExitCode
    }

    $installed = Get-AppxPackage -Name $manifest.identity
    if ($installed -and [version]$installed.Version -ge [version]$manifest.version) {
        Write-Output 'This version or a newer version is already installed. Return to Oceanway Config and click Detect again.'
        exit 0
    }
    Write-Output "Installing $($manifest.name) from the local package..."
    $options = @{ Online = $true; PackagePath = $package; Regions = 'all'; ErrorAction = 'Stop' }
    if ($manifest.license) { $options.LicensePath = Join-Path $PSScriptRoot $manifest.license }
    else { $options.SkipLicense = $true }
    Write-Output 'Registering the offline package and license...'
    Add-AppxProvisionedPackage @options | Out-Null
    Write-Output 'Installing the package for the current Windows user...'
    Add-AppxPackage -Path $package -ErrorAction Stop
    if (!(Get-AppxPackage -Name $manifest.identity)) { throw 'Windows has staged the package. Sign out and sign in, then run detection in Oceanway Config.' }
    Write-Output 'Installation complete. Return to Oceanway Config and click Detect again.'
    exit 0
} catch {
    $installError = $_.Exception.Message
    try { $installError | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'install-error.txt') -Encoding UTF8 } catch {}
    Write-Host $installError -ForegroundColor Red
    Write-Host 'Close the application if it is running, then retry. Existing settings and chat history are kept.'
    exit 1
} finally {
    if ($transcribing) { Stop-Transcript | Out-Null }
}
