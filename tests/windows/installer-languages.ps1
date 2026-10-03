param([string]$Makensis = "$env:LOCALAPPDATA/tauri/NSIS/makensis.exe")
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$directory = Join-Path $workspace ('.tools/installer-languages-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory | Out-Null
$template = Get-Content (Join-Path $workspace 'src-tauri/windows/installer.nsi') -Raw
$map = [regex]::Match($template,'(?s)!macro MapInstallerLanguage uiLanguage.*?!macroend').Value
$detect = [regex]::Match($template,'(?s)!macro DetectSystemInstallerLanguage.*?!macroend').Value
$strings = ([regex]::Matches($template,'(?m)^LangString cgShutdown[^\r\n]+') | ForEach-Object Value) -join "`n"
if (-not $map -or -not $detect -or -not $strings) { throw 'Production locale macros are missing.' }
if ($template -match '!insertmacro MUI_LANGDLL_DISPLAY|!insertmacro MUI_UNGETLANGUAGE') { throw 'Remembered language or language popup remains enabled.' }
$config = Get-Content (Join-Path $workspace 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
if (($config.bundle.windows.nsis.languages -join ',') -ne 'English,SimpChinese' -or $config.bundle.windows.nsis.displayLanguageSelector) { throw 'NSIS languages do not match the requested policy.' }
$cases = @(@(2052,2052),@(1028,2052),@(3076,2052),@(4100,2052),@(5124,2052),@(1033,1033),@(1036,1033),@(1041,1033),@(0,1033))
$checks = ($cases | ForEach-Object { '  !insertmacro MapInstallerLanguage ' + $_[0] + "`n" + '  IntCmp $LANGUAGE ' + $_[1] + ' +2 0 0' + "`n" + '  Abort "Language mapping assertion failed"' }) -join "`n"
$harness = @'
Unicode true
!include MUI2.nsh
Name "Isolated installer locale check"
OutFile "@@EXE@@"
RequestExecutionLevel user
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
@@MAP@@
@@DETECT@@
@@STRINGS@@
Section
@@CHECKS@@
  ; A stale previous language must not influence current-system selection.
  StrCpy $LANGUAGE ${LANG_SIMPCHINESE}
  !insertmacro DetectSystemInstallerLanguage
  FileOpen $0 "@@RESULT@@" w
  FileWrite $0 "$LANGUAGE"
  FileClose $0
SectionEnd
'@
$exe = Join-Path $directory 'locale-check.exe'
$resultPath = Join-Path $directory 'language.txt'
$harness = $harness.Replace('@@EXE@@',$exe).Replace('@@MAP@@',$map).Replace('@@DETECT@@',$detect).Replace('@@STRINGS@@',$strings).Replace('@@CHECKS@@',$checks).Replace('@@RESULT@@',$resultPath)
$source = Join-Path $directory 'locale-check.nsi'
[IO.File]::WriteAllText($source,$harness)
& $Makensis '/V1' '/INPUTCHARSET' 'UTF8' $source
if ($LASTEXITCODE -ne 0) { throw 'Locale harness compilation failed.' }
$process = Start-Process -FilePath $exe -ArgumentList '/S' -WindowStyle Hidden -PassThru
if (-not $process.WaitForExit(15000)) { throw 'Locale harness timed out.' }
$process.Refresh()
if ($process.ExitCode -ne 0) { throw 'Locale mapping assertions failed.' }
Add-Type 'public static class InstallerLocaleCheck { [System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern ushort GetUserDefaultUILanguage(); }'
$systemId = [InstallerLocaleCheck]::GetUserDefaultUILanguage()
$expected = if (($systemId -band 1023) -eq 4) {2052} else {1033}
$actual = [int](Get-Content -LiteralPath $resultPath -Raw)
if ($actual -ne $expected) { throw 'System UI locale selection failed.' }
$evidence = [ordered]@{checkedAt=[DateTime]::UtcNow.ToString('o');mappingCases=$cases.Count;allChineseLocalesSelectSimplifiedChinese=$true;unsupportedLocalesFallBackToEnglish=$true;systemUiLanguageId=$systemId;selectedInstallerLanguageId=$actual;noLanguageDialog=$true;rememberedLanguageDoesNotOverrideSystem=$true}
$evidence | ConvertTo-Json | Set-Content (Join-Path $PSScriptRoot 'installer-languages-evidence.json')
$evidence | ConvertTo-Json

