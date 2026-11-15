param(
    # Git for Windows install the reference copy mirrors
    [string]$Source = 'C:\Program Files\Git'
)
$ErrorActionPreference = 'Stop'

$dst = Join-Path $env:LOCALAPPDATA 'gash-parity\Git'
$markerName = 'gash-parity-copy.txt'
$marker = Join-Path $dst $markerName

if (-not (Test-Path -LiteralPath (Join-Path $Source 'usr\bin\cygpath.exe'))) {
    Write-Host "$Source is not a Git for Windows install, usr\bin\cygpath.exe is missing"
    exit 1
}

# The mirror deletes whatever else the destination holds.
if ((Test-Path -LiteralPath $dst) -and -not (Test-Path -LiteralPath $marker)) {
    if (Get-ChildItem -LiteralPath $dst -Force | Select-Object -First 1) {
        Write-Host "$dst exists and has no $markerName, so it is not a copy made by this script. Remove it first."
        exit 1
    }
}

New-Item -ItemType Directory -Force -Path $dst | Out-Null
Set-Content -LiteralPath $marker -Value "Parity reference copy of $Source"

# /XF keeps the marker out of the purge.
# Exit codes below 8 mean success (files copied, extra files, mismatches).
robocopy $Source $dst /MIR /XF $markerName /NFL /NDL /NJH /NJS /NP /R:1 /W:1 | Out-Null
$code = $LASTEXITCODE
if ($code -ge 8) {
    Write-Host "robocopy failed with exit code $code"
    exit $code
}

Write-Host $dst
exit 0
