@echo off
rem ============================================================================
rem  DFWLogger - INSTALLATEUR UNIQUE
rem  1. Copie l'exe + README dans %LOCALAPPDATA%\Programs\DFWLogger
rem  2. Cree les raccourcis Bureau + menu Demarrer
rem  3. Optionnel : deploie Big Brother (SYSTEM) + Serbert (Ring 0)
rem     - Auto-elevation UAC si necessaire
rem     - Copie le binaire et le pilote dans %ProgramData%\DFWLogger
rem     - Verrouille les permissions NTFS (Admins + SYSTEM : F, Users : RX)
rem ============================================================================
setlocal EnableDelayedExpansion

set "EXE=%~dp0dfwlogger.exe"
set "README=%~dp0README.md"
if not exist "%EXE%" (
    echo [ERREUR] dfwlogger.exe introuvable a cote de ce script ^(extrayez d'abord le zip^).
    pause
    exit /b 1
)

echo ============================================================================
echo   DFWLogger - INSTALLATEUR UNIQUE
echo ============================================================================
echo.

rem --- 1. Installation de l'application (sans elevation) ---
taskkill /F /IM dfwlogger.exe >nul 2>&1
set "DEST=%LOCALAPPDATA%\Programs\DFWLogger"
if not exist "%DEST%" mkdir "%DEST%"
copy /y "%EXE%" "%DEST%\" >nul || (
    echo [ERREUR] copie impossible vers "%DEST%".
    pause
    exit /b 1
)
if exist "%README%" copy /y "%README%" "%DEST%\" >nul

rem --- 2. Raccourcis (Bureau + menu Demarrer) ---
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ws = New-Object -ComObject WScript.Shell; $d = [Environment]::GetFolderPath('Desktop'); if (-not $d) { $d = Join-Path $env:USERPROFILE 'Desktop' }; $t = '%DEST%\dfwlogger.exe'; $s1 = $ws.CreateShortcut((Join-Path $d 'DFWLogger.lnk')); $s1.TargetPath = $t; $s1.IconLocation = ($t + ',0'); $s1.Description = 'Superviseur familial transparent'; $s1.Save(); $m = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\DFWLogger'; if (-not (Test-Path $m)) { [void](New-Item -ItemType Directory -Force -Path $m) }; $s2 = $ws.CreateShortcut((Join-Path $m 'DFWLogger.lnk')); $s2.TargetPath = $t; $s2.IconLocation = ($t + ',0'); $s2.Description = 'Superviseur familial transparent'; $s2.Save()"

echo.
echo [OK] Application installee dans %DEST%.
echo     Raccourcis : Bureau + menu Demarrer

rem --- 3. Optionnel : Mode Ecole (Big Brother SYSTEM + pilote Serbert) ---
echo.
set /p "BB=Installer aussi le Mode Ecole (Big Brother SYSTEM + pilote Serbert) ? (O/N, defaut: N) : "
if /i not "%BB%"=="O" goto :skip_bb

rem Verification des droits administrateur
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Elevation requise. Demande d'elevation UAC...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process cmd -ArgumentList '/c `\"%~f0`\" --elevated' -Verb RunAs"
    goto :end
)

:install_bb
echo.
echo [1/5] Deploiement du binaire dans %ProgramData%\DFWLogger...
set "TARGET_DIR=%ProgramData%\DFWLogger"
if not exist "%TARGET_DIR%" mkdir "%TARGET_DIR%"
copy /y "%EXE%" "%TARGET_DIR%\dfwlogger.exe" >nul

echo [2/5] Recherche du pilote noyau serbert.sys...
if exist "%~dp0serbert.sys" (
    copy /y "%~dp0serbert.sys" "%TARGET_DIR%\serbert.sys" >nul
    echo       Pilote noyau serbert.sys deploye dans %TARGET_DIR%.
) else (
    echo       Pilote noyau non fourni (watchdog Ring 3 actif par defaut).
)

echo [3/5] Installation et demarrage du service DFWLoggerBigBrother...
"%TARGET_DIR%\dfwlogger.exe" --install-bigbrother --quiet

echo [4/5] Verrouillage des permissions NTFS sur %TARGET_DIR%...
icacls "%TARGET_DIR%" /inheritance:d /grant:r "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX" >nul 2>&1

echo [5/5] Mode Ecole installe et actif.
echo.
echo   - Service : DFWLoggerBigBrother (SYSTEM)
echo   - Politique : %TARGET_DIR%\big_brother.json
echo   - Pour configurer les restrictions : onglet "Big Brother" dans l'application.
echo   - Pour desinstaller : lancez "uninstaller.cmd".
goto :end

:skip_bb
echo.
echo Installation terminee (Mode Standard).
echo   Programme : %DEST%\dfwlogger.exe
echo   Terminal  : "%DEST%\dfwlogger.exe" tail 30
echo.
echo   Pour activer le Mode Ecole plus tard :
echo   relancez installer.cmd ou utilisez dfwlogger.exe --install-bigbrother
echo   Pour desinstaller : lancez "uninstaller.cmd".

:end
echo.
echo Remarque : Windows SmartScreen peut demander "Informations
echo complementaires" puis "Executer quand meme" au premier lancement
echo (l'exe n'est pas signe par une autorite payante).
pause
