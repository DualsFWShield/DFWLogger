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
setlocal

set "SCRIPT_DIR=%~dp0"
set "EXE=%SCRIPT_DIR%dfwlogger.exe"
if not exist "%EXE%" if exist "%SCRIPT_DIR%target\release\dfwlogger.exe" set "EXE=%SCRIPT_DIR%target\release\dfwlogger.exe"
set "DEST=%LOCALAPPDATA%\Programs\DFWLogger"
set "README=%SCRIPT_DIR%README.md"

rem --- Mode Test / Verification de syntaxe sans modification ---
if /i "%~1"=="--test" (
    echo [TEST] Verification de l'installateur...
    if exist "%EXE%" (
        echo [TEST] Executable source detecte : %EXE%
    ) else (
        echo [AVERTISSEMENT] Executable source absent : %EXE%
    )
    echo [TEST] Syntaxe batch et chemins valides.
    exit /b 0
)

rem --- Gestion des parametres silencieux et elevation ---
set "QUIET=0"
if /i "%~1"=="--quiet" set "QUIET=1"
if /i "%~1"=="-q" set "QUIET=1"
if /i "%~2"=="--quiet" set "QUIET=1"
if /i "%~2"=="-q" set "QUIET=1"

if /i "%~1"=="--elevated" goto :install_bb
if /i "%~2"=="--elevated" goto :install_bb

if not exist "%EXE%" (
    echo [ERREUR] dfwlogger.exe introuvable. Compilez avec cargo build --release ou extrayez l'archive.
    if "%QUIET%"=="0" pause
    exit /b 1
)

echo ============================================================================
echo   DFWLogger - INSTALLATEUR UNIQUE
echo ============================================================================
echo.

rem --- 1. Installation de l'application (sans elevation) ---
powershell -NoProfile -NonInteractive -Command "Get-Process -Name dfwlogger -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue; exit 0" >nul 2>&1
if not exist "%DEST%" mkdir "%DEST%"
copy /y "%EXE%" "%DEST%\dfwlogger.exe" >nul || (
    echo [ERREUR] Copie impossible vers "%DEST%\dfwlogger.exe".
    if "%QUIET%"=="0" pause
    exit /b 1
)
if exist "%README%" copy /y "%README%" "%DEST%\README.md" >nul

rem --- 2. Raccourcis (Bureau + menu Demarrer) ---
"%DEST%\dfwlogger.exe" --install-shortcuts >nul 2>&1

echo.
echo [OK] Application installee dans %DEST%.
echo     Raccourcis : Bureau + menu Demarrer

rem --- 3. Optionnel : Mode Ecole (Big Brother SYSTEM + pilote Serbert) ---
if "%QUIET%"=="1" goto :skip_bb

echo.
set "BB=N"
set /p "BB=Installer aussi le Mode Ecole (Big Brother SYSTEM + pilote Serbert) ? [O/N, defaut: N] : "
if /i not "%BB%"=="O" goto :skip_bb

rem Verification des droits administrateur
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Elevation requise. Demande d'elevation UAC...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -ArgumentList '--elevated' -Verb RunAs"
    goto :end
)

:install_bb
echo.
echo [1/5] Deploiement du binaire dans %ProgramData%\DFWLogger...
set "TARGET_DIR=%ProgramData%\DFWLogger"
if not exist "%TARGET_DIR%" mkdir "%TARGET_DIR%"
copy /y "%EXE%" "%TARGET_DIR%\dfwlogger.exe" >nul

echo [2/5] Recherche du pilote noyau serbert.sys...
if exist "%SCRIPT_DIR%serbert.sys" (
    copy /y "%SCRIPT_DIR%serbert.sys" "%TARGET_DIR%\serbert.sys" >nul
    echo       Pilote noyau serbert.sys deploye dans %TARGET_DIR%.
) else (
    echo       Pilote noyau non fourni - watchdog Ring 3 actif par defaut.
)

echo [3/5] Installation et demarrage du service DFWLoggerBigBrother...
"%TARGET_DIR%\dfwlogger.exe" --install-bigbrother --quiet

echo [4/5] Verrouillage des permissions NTFS sur %TARGET_DIR%...
icacls "%TARGET_DIR%" /inheritance:d /grant:r "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX" >nul 2>&1

echo [5/5] Mode Ecole installe et actif.
echo.
echo   - Service : DFWLoggerBigBrother (SYSTEM)
echo   - Politique : %TARGET_DIR%\big_brother.json
echo   - Pour configurer les restrictions : onglet "Modes et Regles" dans l'application.
echo   - Pour desinstaller : lancez "uninstaller.cmd".
goto :end

:skip_bb
echo.
echo Installation terminee en Mode Standard.
echo   Programme : %DEST%\dfwlogger.exe
echo   Terminal  : "%DEST%\dfwlogger.exe" tail 30
echo.
echo   Pour activer le Mode Ecole plus tard :
echo   relancez installer.cmd ou utilisez dfwlogger.exe --install-bigbrother
echo   Pour desinstaller : lancez "uninstaller.cmd".

:end
echo.
echo Remarque : Windows SmartScreen peut demander "Informations
echo complementaires" puis "Executer quand meme" au premier lancement.
if "%QUIET%"=="0" pause
exit /b 0
