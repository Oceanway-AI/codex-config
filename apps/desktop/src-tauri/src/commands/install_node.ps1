$ErrorActionPreference = 'Stop'
$owNodeRoot = Join-Path $env:USERPROFILE '.oceanway-config\runtimes\node'
function Test-OwNode {
    if (!(Get-Command node -ErrorAction SilentlyContinue) -or !(Get-Command npm.cmd -ErrorAction SilentlyContinue)) { return $false }
    & node -e 'const v=process.versions.node.split(".").map(Number);process.exit(v[0]>24||v[0]===24&&v[1]>=16?0:1)' 2>$null
    return $LASTEXITCODE -eq 0
}
if (!(Test-OwNode)) {
    $env:PATH = "$owNodeRoot;$env:PATH"
    if (!(Test-OwNode)) {
        Write-Output 'Installing the Node.js runtime required by this app...'
        $owArch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') { 'arm64' } else { 'x64' }
        $owBase = 'https://nodejs.org/dist/latest-v24.x'
        $owSums = (Invoke-WebRequest "$owBase/SHASUMS256.txt" -UseBasicParsing -TimeoutSec 60).Content
        $owMatch = [regex]::Match($owSums, "(?m)^([a-f0-9]{64})\s+(node-v24\.[0-9.]+-win-$owArch\.zip)\s*$")
        if (!$owMatch.Success) { throw 'No matching official Node.js archive' }
        $owTemp = Join-Path ([IO.Path]::GetTempPath()) ('oceanway-node-' + [guid]::NewGuid())
        New-Item -ItemType Directory -Path $owTemp | Out-Null
        try {
            $owZip = Join-Path $owTemp $owMatch.Groups[2].Value
            Invoke-WebRequest "$owBase/$($owMatch.Groups[2].Value)" -UseBasicParsing -TimeoutSec 300 -OutFile $owZip
            if ((Get-FileHash -LiteralPath $owZip -Algorithm SHA256).Hash -ne $owMatch.Groups[1].Value) { throw 'Node.js download checksum mismatch' }
            Expand-Archive -LiteralPath $owZip -DestinationPath $owTemp
            $owExtracted = Join-Path $owTemp ([IO.Path]::GetFileNameWithoutExtension($owZip))
            New-Item -ItemType Directory -Path $owNodeRoot -Force | Out-Null
            Get-ChildItem -LiteralPath $owExtracted | Copy-Item -Destination $owNodeRoot -Recurse -Force
            if (!(Test-OwNode)) { throw 'Installed Node.js runtime cannot start' }
        } finally {
            $owResolved = [IO.Path]::GetFullPath($owTemp)
            $owTempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
            if (!$owResolved.StartsWith($owTempParent, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($owResolved) -notmatch '^oceanway-node-[0-9a-f-]{36}$') { throw 'Unexpected installer temporary directory' }
            Remove-Item -LiteralPath $owResolved -Recurse -Force
        }
    }
    $env:NPM_CONFIG_PREFIX = $owNodeRoot
    $owUserPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
    if (($owUserPath -split ';') -notcontains $owNodeRoot) {
        [Environment]::SetEnvironmentVariable('PATH', "$owNodeRoot;$owUserPath", 'User')
    }
}
& cmd.exe /d /c $owInstallCommand
exit $LASTEXITCODE
