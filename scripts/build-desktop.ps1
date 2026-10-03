[CmdletBinding()]
param(
    [ValidateSet('Check', 'Dev', 'Bundle', 'Mcp')]
    [string]$Mode = 'Check',
    [string]$CodeGraphBundle,
    [string]$CodeGraphEntry
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectDirectory = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectDirectory

# Both x64 and ARM64 use the native Rust host installed on this machine.
$cargoDirectory = Join-Path $env:USERPROFILE '.cargo/bin'
if (Test-Path -LiteralPath (Join-Path $cargoDirectory 'cargo.exe')) {
    $env:PATH = "$cargoDirectory;$env:PATH"
}
foreach ($program in @('cargo', 'rustc', 'node', 'npm.cmd')) {
    if (-not (Get-Command $program -ErrorAction SilentlyContinue)) {
        throw "Missing $program. Install native Rust stable, Node.js 22+, and the matching MSVC C++ build tools with Windows SDK."
    }
}
function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE." }
}
if (-not (Test-Path -LiteralPath 'node_modules')) { Invoke-Checked 'npm.cmd' @('ci') }
if ($Mode -ne 'Mcp') {
    Invoke-Checked 'node' @('scripts/build-desktop.mjs', $Mode)
    return
}
Invoke-Checked 'node' @('scripts/build-desktop.mjs', 'Prepare')
Invoke-Checked 'cargo' @('build', '--locked', '-p', 'project-gateway', '--examples')
$probeArguments = @('tests/m0/probe.cjs')
if ($CodeGraphBundle) { $probeArguments += $CodeGraphBundle }
Invoke-Checked 'node' $probeArguments
$smokeArguments = @('tests/m0/gateway-smoke.cjs')
if ($CodeGraphEntry) { $smokeArguments += $CodeGraphEntry }
Invoke-Checked 'node' $smokeArguments
$httpArguments = @('tests/m0/http-direct-smoke.cjs')
if ($CodeGraphEntry) { $httpArguments += $CodeGraphEntry }
Invoke-Checked 'node' $httpArguments
