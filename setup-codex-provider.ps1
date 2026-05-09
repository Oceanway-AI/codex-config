param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $ArgsForPython
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$PythonScript = Join-Path $ScriptDir "setup-codex-provider.py"

$Python = Get-Command py -ErrorAction SilentlyContinue
if ($Python) {
    & py -3 $PythonScript @ArgsForPython
    exit $LASTEXITCODE
}

$Python = Get-Command python -ErrorAction SilentlyContinue
if ($Python) {
    & python $PythonScript @ArgsForPython
    exit $LASTEXITCODE
}

throw "Python 3 was not found. Install Python 3 or run this script from a shell where python is available."
