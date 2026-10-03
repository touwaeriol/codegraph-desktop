param([Parameter(Mandatory = $true)][string]$FixtureManifest, [switch]$TestLockedConnector)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$fixture = Get-Content -LiteralPath $FixtureManifest -Raw | ConvertFrom-Json
if ($fixture.name -notmatch '^CodeGraph Upgrade Check [a-f0-9]{32}$' -or $fixture.identifier -notmatch '^ai\.codegraph\.upgradecheck\.[a-f0-9]{32}$') { throw 'Only isolated test identities are permitted.' }
$root = (Resolve-Path -LiteralPath $fixture.root).Path
if (-not $root.StartsWith((Join-Path $workspace '.tools/installer-upgrade-'), [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture must be inside the workspace test directory.' }
$install = Join-Path $root 'install'
$registry = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$($fixture.name)"
if (Test-Path -LiteralPath $registry) { throw 'Test identity is already installed.' }
function Run-Installer([string]$path, [string[]]$arguments) {
    $process = Start-Process -FilePath $path -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(180000)) { Stop-Process -Id $process.Id -Force; throw 'Test installer timed out.' }
    $process.Refresh()
    if ($process.ExitCode -ne 0) { throw "Installer exited with $($process.ExitCode)" }
}
$first = Join-Path $workspace "target/release/bundle/nsis/$($fixture.name)_0.1.0_x64-setup.exe"
$second = Join-Path $workspace "target/release/bundle/nsis/$($fixture.name)_0.1.1_x64-setup.exe"
Run-Installer $first @('/S', '/NS', "/D=$install")
if ((Get-ItemProperty -LiteralPath $registry).DisplayVersion -ne '0.1.0') { throw 'Initial version not registered.' }
$app = Join-Path $install 'codegraph-desktop.exe'
$connector = Join-Path $install 'cg-mcp-connector.exe'
$uninstaller = Join-Path $install 'uninstall.exe'
$appHash = (Get-FileHash -LiteralPath $app).Hash
$connectorHash = (Get-FileHash -LiteralPath $connector).Hash
$realUninstaller = Join-Path $root 'original-uninstall.exe'
Copy-Item -LiteralPath $uninstaller -Destination $realUninstaller
$probe = Join-Path $root 'uninstall-probe.rs'
[IO.File]::WriteAllText($probe, 'fn main() { let p = std::env::current_exe().unwrap().with_file_name("old-uninstaller-was-run"); std::fs::write(p, b"unexpected").unwrap(); std::process::exit(77); }')
& "$env:USERPROFILE/.cargo/bin/rustc.exe" --crate-name uninstall_probe $probe -o $uninstaller
if ($LASTEXITCODE -ne 0) { throw 'Cannot compile uninstaller probe.' }
$data = Join-Path $env:APPDATA $fixture.identifier
New-Item -ItemType Directory -Path $data -Force | Out-Null
$sentinel = Join-Path $data 'preserve-settings.txt'
[IO.File]::WriteAllText($sentinel, 'preserve-user-data')
# Force both payload hashes to differ, proving that the second installer replaces both.
[IO.File]::WriteAllText($app, 'old-main-binary')
[IO.File]::WriteAllText($connector, 'old-connector-binary')
$lockedProcess = $null
$lockedMainProcess = $null
$unrelatedProcess = $null
if ($TestLockedConnector) {
    $lockSource = Join-Path $root 'connector-lock.rs'
    [IO.File]::WriteAllText($lockSource, 'fn main() { loop { std::thread::sleep(std::time::Duration::from_secs(1)); } }')
    & "$env:USERPROFILE/.cargo/bin/rustc.exe" --crate-name connector_lock $lockSource -o $connector
    if ($LASTEXITCODE -ne 0) { throw 'Cannot compile file-lock fixture.' }
    $otherDirectory = Join-Path $root 'unrelated-install'
    New-Item -ItemType Directory -Path $otherDirectory -Force | Out-Null
    $otherConnector = Join-Path $otherDirectory 'cg-mcp-connector.exe'
    Copy-Item -LiteralPath $connector -Destination $otherConnector -Force
    Copy-Item -LiteralPath $connector -Destination $app -Force
    $lockedMainProcess = Start-Process -FilePath $app -WindowStyle Hidden -PassThru
    $lockedProcess = Start-Process -FilePath $connector -WindowStyle Hidden -PassThru
    $unrelatedProcess = Start-Process -FilePath $otherConnector -WindowStyle Hidden -PassThru
}
try {
    # No /UPDATE or /D: default installer behavior must locate and update the old install.
    Run-Installer $second @('/S', '/NS')
    if ($TestLockedConnector) {
        if (-not $lockedProcess.WaitForExit(5000)) { throw 'Installed connector was not stopped.' }
        if (-not $lockedMainProcess.WaitForExit(5000)) { throw 'Installed main process was not stopped.' }
        $unrelatedProcess.Refresh()
        if ($unrelatedProcess.HasExited) { throw 'Unrelated same-name connector was stopped.' }
    }
    if (Test-Path -LiteralPath (Join-Path $install 'old-uninstaller-was-run')) { throw 'Old uninstaller was invoked.' }
    if ((Get-ItemProperty -LiteralPath $registry).DisplayVersion -ne '0.1.1') { throw 'Upgrade version not registered.' }
    if ((Get-FileHash -LiteralPath $app).Hash -ne $appHash) { throw 'Main binary not replaced in original directory.' }
    if ((Get-FileHash -LiteralPath $connector).Hash -ne $connectorHash) { throw 'Connector not replaced.' }
    if ([IO.File]::ReadAllText($sentinel) -ne 'preserve-user-data') { throw 'User data changed.' }
    Run-Installer $uninstaller @('/S', "_?=$install")
    if (Test-Path -LiteralPath $registry) { throw 'Independent uninstall registration remains.' }
    if ((Test-Path -LiteralPath $app) -or (Test-Path -LiteralPath $connector)) { throw 'Independent uninstall left payloads.' }
    if ([IO.File]::ReadAllText($sentinel) -ne 'preserve-user-data') { throw 'Default uninstall unexpectedly removed user data.' }
    $result = [ordered]@{ checkedAt = [DateTime]::UtcNow.ToString('o'); scope = 'Isolated NSIS identity, silent 0.1.0 -> 0.1.1 using current template and release payload; no real user installation changed'; oldUninstallerNeverInvoked = $true; originalCustomDirectoryReused = $true; mainAndConnectorReplaced = $true; userDataPreserved = $true; independentUninstallPassed = $true; productionIdentityUpgradeTested = $false }
    $result.lockedConnectorStopped = [bool]$TestLockedConnector
    $result.lockedMainProcessStopped = [bool]$TestLockedConnector
    $result.unrelatedSameNameProcessPreserved = [bool]$TestLockedConnector
    [IO.File]::WriteAllText((Join-Path $workspace 'tests/windows/in-place-upgrade-evidence.json'), ($result | ConvertTo-Json))
    $result | ConvertTo-Json
} finally {
    foreach ($owned in @($lockedProcess, $lockedMainProcess, $unrelatedProcess)) {
        if ($null -ne $owned) { $owned.Refresh(); if (-not $owned.HasExited) { Stop-Process -Id $owned.Id -Force } }
    }
    # Only clean this isolated identity; do not touch the real CodeGraph installation.
    if (Test-Path -LiteralPath $registry) {
        Copy-Item -LiteralPath $realUninstaller -Destination $uninstaller -Force
        Run-Installer $uninstaller @('/S', "_?=$install")
    }
    Remove-Item -LiteralPath $sentinel -ErrorAction SilentlyContinue
    if ((Test-Path -LiteralPath $data) -and @(Get-ChildItem -LiteralPath $data -Force).Count -eq 0) { Remove-Item -LiteralPath $data }
    $installLocationKey = "HKCU:\Software\codegraph\$($fixture.name)"
    if (Test-Path -LiteralPath $installLocationKey) { Remove-Item -LiteralPath $installLocationKey }
}
