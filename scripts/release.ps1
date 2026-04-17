Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $PSCommandPath
& node (Join-Path $scriptDir "release.mjs") @args
exit $LASTEXITCODE
