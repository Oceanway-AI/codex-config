Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OceanWayArguments {
    [DllImport("shell32.dll", SetLastError=true)]
    static extern IntPtr CommandLineToArgvW([MarshalAs(UnmanagedType.LPWStr)] string command, out int argc);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr pointer);
    public static string[] Parse(string command) {
        if (String.IsNullOrEmpty(command)) return new string[0];
        int count;
        IntPtr pointer = CommandLineToArgvW(command, out count);
        if (pointer == IntPtr.Zero) throw new InvalidOperationException("Cannot parse process arguments");
        try {
            string[] result = new string[count];
            for (int i=0; i<count; i++) result[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(pointer, i*IntPtr.Size));
            return result;
        } finally { LocalFree(pointer); }
    }
}
'@
function Get-DesktopProfile($command) {
    $arguments = [OceanWayArguments]::Parse($command)
    for ($i=0; $i -lt $arguments.Length; $i++) {
        if ($arguments[$i].StartsWith('--user-data-dir=', [StringComparison]::OrdinalIgnoreCase)) {
            return $arguments[$i].Substring('--user-data-dir='.Length)
        }
        if ($arguments[$i] -ieq '--user-data-dir') {
            if ($i+1 -lt $arguments.Length) { return $arguments[$i+1] }
            return '<invalid>'
        }
    }
    return $null
}

function Get-DesktopVersion($path) {
    if (!$path) { return $null }
    try {
        $absolute = [IO.Path]::GetFullPath($path)
        foreach ($package in @(Get-AppxPackage -Name OpenAI.Codex -ErrorAction SilentlyContinue)) {
            $prefix = [IO.Path]::GetFullPath($package.InstallLocation).TrimEnd('\') + '\'
            if ($absolute.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                return $package.Version.ToString()
            }
        }
        # Electron's EXE version is Chromium's. Read only packaged app metadata
        # for standalone installs; never execute or modify archive contents.
        $archive = Join-Path ([IO.Path]::GetDirectoryName($absolute)) 'resources\app.asar'
        $stream = [IO.File]::OpenRead($archive)
        $reader = [IO.BinaryReader]::new($stream)
        try {
            if ($reader.ReadUInt32() -ne 4) { return $null }
            $headerSize = $reader.ReadUInt32()
            $null = $reader.ReadUInt32()
            $jsonSize = $reader.ReadUInt32()
            if ($headerSize -gt 16777216 -or $jsonSize -le 0 -or $jsonSize -gt ($headerSize - 8)) { return $null }
            $header = [Text.Encoding]::UTF8.GetString($reader.ReadBytes($jsonSize)) | ConvertFrom-Json
            $entry = $header.files.'package.json'
            [long]$offset = 0
            if (!$entry -or $entry.link -or $entry.unpacked -or $entry.size -le 0 -or $entry.size -gt 1048576 -or
                ![long]::TryParse([string]$entry.offset, [ref]$offset) -or $offset -lt 0) { return $null }
            $position = 8L + $headerSize + $offset
            if ($position -gt $stream.Length -or $entry.size -gt ($stream.Length - $position)) { return $null }
            $stream.Position = $position
            $metadata = [Text.Encoding]::UTF8.GetString($reader.ReadBytes([int]$entry.size)) | ConvertFrom-Json
            if ($metadata.name -ne 'openai-codex-electron' -or $metadata.version -notmatch '^\d+\.\d+\.\d+(?:\.\d+)?$') { return $null }
            return [string]$metadata.version
        } finally { $reader.Dispose(); $stream.Dispose() }
    } catch { return $null }
}
