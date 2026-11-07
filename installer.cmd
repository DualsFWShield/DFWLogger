@echo off
rem ============================================================
rem  DFWLogger - installation utilisateur (aucune elevation)
rem  Copie l'exe dans %LOCALAPPDATA%\Programs\DFWLogger et crée
rem  des raccourcis Bureau + menu Démarrer. L'application reste
rem  100%% visible et transparente.
rem ============================================================
setlocal
set "EXE=%~dp0dfwlogger.exe"
set "README=%~dp0README.md"
if not exist "%EXE%" (
  echo [ERREUR] dfwlogger.exe introuvable a cote de ce script ^(extrait d'abord le zip^).
  exit /b 1
)
set "DEST=%LOCALAPPDATA%\Programs\DFWLogger"
if not exist "%DEST%" mkdir "%DEST%"
copy /y "%EXE%" "%DEST%\" >nul || (
  echo [ERREUR] copie impossible vers "%DEST%".
  exit /b 1
)
if exist "%README%" copy /y "%README%" "%DEST%\" >nul

rem --- raccourcis (Bureau + menu Demarrer) via WScript.Shell ---
powershell -NoProfile -Command "$ws = New-Object -ComObject WScript.Shell; $desktop = [Environment]::GetFolderPath('Desktop'); $lnk1 = $ws.CreateShortcut((Join-Path $desktop 'DFWLogger.lnk')); $lnk1.TargetPath = '%DEST%\dfwlogger.exe'; $lnk1.IconLocation = '%DEST%\dfwlogger.exe,0'; $lnk1.Description = 'Superviseur familial transparent'; $lnk1.Save(); $smenu = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\DFWLogger'; if (!(Test-Path $smenu)) { New-Item -ItemType Directory -Force $smenu | Out-Null }; $lnk2 = $ws.CreateShortcut((Join-Path $smenu 'DFWLogger.lnk')); $lnk2.TargetPath = '%DEST%\dfwlogger.exe'; $lnk2.IconLocation = '%DEST%\dfwlogger.exe,0'; $lnk2.Description = 'Superviseur familial transparent'; $lnk2.Save()"

echo.
echo Installation terminee.
echo   Programme : %DEST%\dfwlogger.exe
echo   Raccourcis : Bureau + menu Demarrer -^> DFWLogger
echo   Terminal : "%DEST%\dfwlogger.exe" tail 30
echo.
echo Remarque : Windows SmartScreen peut demander "Informations
echo complementaires" puis "Executer quand meme" au premier lancement
echo (l'exe n'est pas signe par une autorite payante).
pause
