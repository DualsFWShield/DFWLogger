@echo off
rem ============================================================================
rem  DFWLogger - Deploiement de Big Brother & Serbert (SYSTEM + Ring 0)
rem ============================================================================
setlocal EnableDelayedExpansion

net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Elevation requise. Demande d'elevation UAC...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process cmd -ArgumentList '/c `\"%~f0`\" %*' -Verb RunAs"
    exit /b
)

echo ============================================================================
echo   DFWLogger - Deploiement de Big Brother (SYSTEM) et Serbert (Noyau)
echo ============================================================================
echo.

set "EXE=%~dp0dfwlogger.exe"
if not exist "%EXE%" (
    echo [ERREUR] dfwlogger.exe introuvable a cote de ce script.
    pause
    exit /b 1
)

set "TARGET_DIR=%ProgramData%\DFWLogger"
if not exist "%TARGET_DIR%" mkdir "%TARGET_DIR%"
copy /y "%EXE%" "%TARGET_DIR%\dfwlogger.exe" >nul

rem Copie du pilote noyau serbert.sys si present
if exist "%~dp0serbert.sys" (
    copy /y "%~dp0serbert.sys" "%TARGET_DIR%\serbert.sys" >nul
    echo [1/4] Pilote noyau serbert.sys deploye dans %TARGET_DIR%.
) else (
    echo [1/4] Pilote noyau serbert.sys non fourni (watchdog Ring 3 actif).
)

echo [2/4] Binaire Big Brother deploye dans %TARGET_DIR%\dfwlogger.exe.
echo [3/4] Installation et demarrage du service DFWLoggerBigBrother...
"%TARGET_DIR%\dfwlogger.exe" --install-bigbrother --quiet

echo [4/4] Verrouillage des permissions NTFS sur %TARGET_DIR%...
icacls "%TARGET_DIR%" /inheritance:d /grant:r "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX" >nul 2>&1

echo.
echo ============================================================================
echo   [SUCCES] Big Brother est installe et actif.
echo   - Service : DFWLoggerBigBrother (SYSTEM)
echo   - Politique : %TARGET_DIR%\big_brother.json
echo   - Journal d'activite : %TARGET_DIR%\big_brother.log
echo.
echo   Pour configurer les restrictions : lancez l'onglet "Big Brother".
echo   Pour desinstaller : lancez "uninstall_big_brother.cmd".
echo ============================================================================
echo.
pause

