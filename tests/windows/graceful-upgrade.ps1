param([string]$Makensis = "$env:LOCALAPPDATA/tauri/NSIS/makensis.exe")
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$fixture = Join-Path $workspace ('.tools/upgrade-shutdown-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$source = Join-Path $fixture 'fixture.rs'
@'
use std::{fs,process::{Command,Stdio},time::Duration};
fn main(){let exe=std::env::current_exe().unwrap();let root=exe.parent().unwrap();let args:Vec<_>=std::env::args().collect();if args.iter().any(|a|a=="--shutdown-for-update"){fs::write(root.join("request-received"),b"yes").unwrap();return;}if args.iter().any(|a|a=="--worker"){loop{std::thread::sleep(Duration::from_secs(1));}}let mut child=Command::new(&exe).arg("--worker").stdin(Stdio::null()).stdout(Stdio::null()).spawn().unwrap();fs::write(root.join("child.pid"),child.id().to_string()).unwrap();loop{if root.join("request-received").exists(){child.kill().unwrap();child.wait().unwrap();fs::write(root.join("codegraph-stopped"),b"yes").unwrap();std::thread::sleep(Duration::from_millis(300));fs::write(root.join("app-exiting"),b"yes").unwrap();return;}std::thread::sleep(Duration::from_millis(10));}}
'@ | Set-Content -LiteralPath $source
$app = Join-Path $fixture 'codegraph-desktop.exe'
& "$env:USERPROFILE/.cargo/bin/rustc.exe" --crate-name shutdown_fixture $source -o $app
if ($LASTEXITCODE -ne 0) { throw 'Cannot compile shutdown fixture.' }
$connector = Join-Path $fixture 'cg-mcp-connector.exe'
Copy-Item -LiteralPath $app -Destination $connector
$otherDir = Join-Path $fixture 'unrelated'
New-Item -ItemType Directory -Path $otherDir | Out-Null
$otherExe = Join-Path $otherDir 'codegraph-desktop.exe'
Copy-Item -LiteralPath $app -Destination $otherExe
$template = Get-Content (Join-Path $workspace 'src-tauri/windows/installer.nsi') -Raw
$macro = [regex]::Match($template, '(?s)!macro GracefulShutdownForUpdate executablePath.*?!macroend').Value
if (-not $macro) { throw 'Production graceful-shutdown macro not found.' }
$includes = Join-Path $workspace 'target/release/nsis/x64'
$installer = Join-Path $fixture 'update-check.exe'
$script = @'
Unicode true
!include MUI2.nsh
!include "Win\RestartManager.nsh"
!include "FileFunc.nsh"
!include "utils.nsh"
!addplugindir "@@PLUGINS@@"
Name "Isolated shutdown regression"
OutFile "@@OUTFILE@@"
RequestExecutionLevel user
Var PassiveMode
LangString appRunning 1033 "Application running"
LangString appRunningOkKill 1033 "Stop application?"
LangString failedToKillApp 1033 "Stop failed"
@@MACRO@@
Section
  StrCpy $PassiveMode 1
  !insertmacro GracefulShutdownForUpdate "@@APP@@"
  !insertmacro CheckIfAppIsRunning "@@APP@@" "Fixture app"
  !insertmacro CheckIfAppIsRunning "@@CONNECTOR@@" "Fixture connector"
  FileOpen $0 "@@MARKER@@" w
  FileWrite $0 "payload-write-after-shutdown"
  FileClose $0
SectionEnd
'@
$script = $script.Replace('@@OUTFILE@@',$installer).Replace('@@MACRO@@',$macro).Replace('@@PLUGINS@@',(Join-Path (Split-Path $Makensis) 'Plugins/x86-unicode/additional')).Replace('@@APP@@',$app).Replace('@@CONNECTOR@@',$connector).Replace('@@MARKER@@',(Join-Path $fixture 'payload-written'))
$nsi = Join-Path $fixture 'check.nsi'
[IO.File]::WriteAllText($nsi,$script)
& $Makensis '/V2' "/X!addincludedir $includes" $nsi
if ($LASTEXITCODE -ne 0) { throw 'Cannot compile production shutdown macro.' }
$main = Start-Process -FilePath $app -WindowStyle Hidden -PassThru
$lockedConnector = Start-Process -FilePath $connector -ArgumentList '--worker' -WindowStyle Hidden -PassThru
$other = Start-Process -FilePath $otherExe -ArgumentList '--worker' -WindowStyle Hidden -PassThru
$workerId = $null
try {
    for($i=0;$i -lt 100 -and -not (Test-Path (Join-Path $fixture 'child.pid'));$i++){Start-Sleep -Milliseconds 50}
    $workerId = [int](Get-Content (Join-Path $fixture 'child.pid'))
    $update = Start-Process -FilePath $installer -ArgumentList '/S' -WindowStyle Hidden -PassThru
    if(-not $update.WaitForExit(90000)){throw 'Update fixture timed out.'}
    $update.Refresh();if($update.ExitCode -ne 0){throw 'Update fixture failed.'}
    foreach($marker in @('request-received','codegraph-stopped','app-exiting','payload-written')){if(-not(Test-Path (Join-Path $fixture $marker))){throw "Missing ordering evidence: $marker"}}
    if(-not $main.WaitForExit(2000) -or -not $lockedConnector.WaitForExit(2000)){throw 'Installed processes remain.'}
    if(Get-Process -Id $workerId -ErrorAction SilentlyContinue){throw 'Managed worker remains.'}
    $other.Refresh();if($other.HasExited){throw 'Unrelated installation was stopped.'}
    $times=@('codegraph-stopped','app-exiting','payload-written') | ForEach-Object {(Get-Item (Join-Path $fixture $_)).LastWriteTimeUtc}
    if($times[0] -gt $times[1] -or $times[1] -gt $times[2]){throw 'Shutdown/write order violated.'}
    $evidence=[ordered]@{checkedAt=[DateTime]::UtcNow.ToString('o');scope='Production NSIS graceful macro with isolated cooperative app/worker fixture';gracefulRequestDelivered=$true;workerStoppedBeforeAppExit=$true;applicationExitedBeforePayloadWrite=$true;legacyConnectorStopped=$true;unrelatedInstallationPreserved=$true}
    $evidence | ConvertTo-Json | Set-Content (Join-Path $PSScriptRoot 'graceful-upgrade-evidence.json')
    $evidence | ConvertTo-Json
} finally {
    foreach($process in @($main,$lockedConnector,$other)){if($process){$process.Refresh();if(-not $process.HasExited){Stop-Process -Id $process.Id -Force}}}
    if($workerId -and (Get-Process -Id $workerId -ErrorAction SilentlyContinue)){Stop-Process -Id $workerId -Force}
}
