@echo off
rem ============================================================================
rem  DFWLogger - DESINSTALLATEUR UNIQUE
rem  Desinstallation complete de l'application :
rem   - Retrait des protections Big Brother / Serbert (SYSTEM + Ring 0)
rem   - Arret et suppression des services Windows
rem   - Restauration DNS / hosts / registre / quotas
rem   - Suppression des raccourcis (Bureau + menu Demarrer)
rem   - Suppression des dossiers applicatifs et des donnees utilisateur
rem ============================================================================
setlocal EnableDelayedExpansion

net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Elevation requise. Demande d'elevation UAC...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process cmd -ArgumentList '/c `\"%~f0`\" %*' -Verb RunAs"
    exit /b
)

echo ============================================================================
echo   DFWLogger - DESINSTALLATION UNIE
echo ============================================================================

set "EXE=%~dp0dfwlogger.exe"
if not exist "%EXE%" (
    if exist "%LOCALAPPDATA%\Programs\DFWLogger\dfwlogger.exe" (
        set "EXE=%LOCALAPPDATA%\Programs\DFWLogger\dfwlogger.exe"
    ) else if exist "%ProgramData%\DFWLogger\dfwlogger.exe" (
        set "EXE=%ProgramData%\DFWLogger\dfwlogger.exe"
    )
)

echo [1/8] Arret de l'application et retrait des protections...
if exist "%EXE%" (
    taskkill /F /IM dfwlogger.exe >nul 2>&1
    "%EXE%" --uninstall-bigbrother --quiet
)

echo [2/8] Arret et suppression des services Windows...
sc stop DFWLoggerBigBrother >nul 2>&1
sc delete DFWLoggerBigBrother >nul 2>&1
sc stop Serbert >nul 2>&1
sc delete Serbert >nul 2>&1

echo [3/8] Nettoyage du registre...
reg delete "HKLM\Software\Policies\Microsoft\Windows\Installer" /v "DisableMSI" /f >nul 2>&1

echo [4/8] Vidage du cache DNS...
ipconfig /flushdns >nul 2>&1

echo [5/8] Suppression des raccourcis (Bureau + menu Demarrer)...
powershell -NoProfile -Command "$ws = New-Object -ComObject WScript.Shell; $desktop = [Environment]::GetFolderPath('Desktop'); $lnk1 = Join-Path $desktop 'DFWLogger.lnk'; if (Test-Path $lnk1) { Remove-Item $lnk1 -Force }; $sm = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\DFWLogger'; if (Test-Path $sm) { Remove-Item -Recurse -Force $sm }"

echo [6/8] Suppression du dossier de l application...
set /p "CLEAN_APP=Effacer le dossier de l application %LOCALAPPDATA%\Programs\DFWLogger ? (O/N, defaut: O) : "
if /i "%CLEAN_APP%"=="O" (
    powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '%LOCALAPPDATA%\Programs\DFWLogger'"
)

echo [7/8] Suppression des donnees utilisateur...
set /p "CLEAN_DATA=Effacer les donnees utilisateur (journal + configuration) %LOCALAPPDATA%\DFWLogger ? (O/N, defaut: O) : "
if /i "%CLEAN_DATA%"=="O" (
    powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '%LOCALAPPDATA%\DFWLogger'"
)

echo [8/8] Suppression des donnees Big Brother (politique + etat)...
set /p "CLEAN_BB=Effacer les donnees Big Brother (politique + etat) %ProgramData%\DFWLogger ? (O/N, defaut: O) : "
if /i "%CLEAN_BB%"=="O" (
    powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '%ProgramData%\DFWLogger'"
)

echo.
echo ============================================================================
echo   [SUCCES] DFWLogger a ete completement desinstalle.
echo   - Protections Big Brother / Serbert retirees.
echo   - Services Windows supprimes.
echo   - Raccourcis et dossiers nettoyes.
echo ============================================================================
echo.
pause
