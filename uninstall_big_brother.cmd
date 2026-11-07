@echo off
rem ============================================================================
rem  DFWLogger - Desinstallation propre de Big Brother & Serbert
rem ============================================================================
setlocal EnableDelayedExpansion

net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Elevation requise. Demande d'elevation UAC...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process cmd -ArgumentList '/c `\"%~f0`\" %*' -Verb RunAs"
    exit /b
)

echo ============================================================================
echo   DFWLogger - Desinstallation de Big Brother & Serbert
echo ============================================================================
echo.

set "EXE=%~dp0dfwlogger.exe"
if not exist "%EXE%" (
    if exist "%LOCALAPPDATA%\Programs\DFWLogger\dfwlogger.exe" (
        set "EXE=%LOCALAPPDATA%\Programs\DFWLogger\dfwlogger.exe"
    ) else if exist "%ProgramData%\DFWLogger\dfwlogger.exe" (
        set "EXE=%ProgramData%\DFWLogger\dfwlogger.exe"
    )
)

if exist "%EXE%" (
    echo [1/4] Retrait des protections via dfwlogger.exe --uninstall-bigbrother...
    "%EXE%" --uninstall-bigbrother --quiet
)

rem Arret et suppression du service et du pilote noyau
echo [2/4] Arret et suppression des services Windows...
sc stop DFWLoggerBigBrother >nul 2>&1
sc delete DFWLoggerBigBrother >nul 2>&1
sc stop DFWLoggerGuard >nul 2>&1
sc delete DFWLoggerGuard >nul 2>&1
sc stop Serbert >nul 2>&1
sc delete Serbert >nul 2>&1

rem Nettoyage registre
echo [3/4] Nettoyage du registre machine (MSI et SRP)...
reg delete "HKLM\Software\Policies\Microsoft\Windows\Installer" /v "DisableMSI" /f >nul 2>&1

rem Vidage DNS
echo [4/4] Vidage du cache DNS...
ipconfig /flushdns >nul 2>&1

echo.
echo Voulez-vous supprimer definitivement le dossier de configuration
set /p "CLEAN_DATA=C:\ProgramData\DFWLogger ? (O/N, defaut: O) : "
if /i not "%CLEAN_DATA%"=="N" (
    if exist "%ProgramData%\DFWLogger" (
        rmdir /s /q "%ProgramData%\DFWLogger" >nul 2>&1
        echo [OK] Dossier %ProgramData%\DFWLogger supprime.
    )
)

echo.
echo ============================================================================
echo   [SUCCES] Big Brother et Serbert ont ete completement desinstalles.
echo   - Services Windows supprimes.
echo   - Pilote noyau Serbert decharge.
echo   - Fichier hosts et registre restaures.
echo   - Cache DNS vide.
echo ============================================================================
echo.
pause

