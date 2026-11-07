@echo off
rem ============================================================
rem  DFWLogger - INSTALLATEUR UNIQUE
rem  - Copie l'exe + README dans %LOCALAPPDATA%\Programs\DFWLogger
rem  - Cree les raccourcis Bureau + menu Demarrer
rem  - Optionnel : installe le Mode Ecole (Big Brother SYSTEM)
rem    via le flag --install-bigbrother (auto-eleve si besoin).
rem ============================================================
setlocal

set "EXE=%~dp0dfwlogger.exe"
set "README=%~dp0README.md"
if not exist "%EXE%" (
  echo [ERREUR] dfwlogger.exe introuvable a cote de ce script ^(^extrait d'abord le zip^).
  exit /b 1
)

echo ============================================================================
echo   DFWLogger - INSTALLATEUR UNIQUE
echo ============================================================================
echo.

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
echo [OK] Application installee dans %DEST%.

rem --- Optionnel : Mode Ecole (Big Brother SYSTEM + pilote Serbert) ---
echo.
set /p "BB=Installer aussi le Mode Ecole (Big Brother SYSTEM + pilote Serbert) (O/N, defaut: N) : "
if /i "%BB%"=="O" (
  echo [1/2] Installation du Mode Ecole (service DFWLoggerBigBrother SYSTEM)...
  "%DEST%\dfwlogger.exe" --install-bigbrother --quiet
  echo [2/2] Installation du Mode Ecole terminee.
)

echo.
echo Installation terminee (Mode Familial standard).
echo   Programme : %DEST%\dfwlogger.exe
echo   Raccourcis : Bureau + menu Demarrer -^> DFWLogger
echo   Terminal : "%DEST%\dfwlogger.exe" tail 30
echo.
echo   Pour les ecoles et administrateurs de parc :
echo   Pour installer Big Brother (SYSTEM) et Serbert (Noyau) :
echo   lancez "install_big_brother.cmd" (ou dfwlogger.exe --install-bigbrother).
echo   Pour le desinstaller proprement : lancez "uninstall_big_brother.cmd".
echo.
echo Remarque : Windows SmartScreen peut demander "Informations
echo complementaires" puis "Executer quand meme" au premier lancement
echo (l'exe n'est pas signe par une autorite payante).
pause
