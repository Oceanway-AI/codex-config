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
