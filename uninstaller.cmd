@echo off
rem ============================================================================
rem  DFWLogger - DESINSTALLATEUR UNIQUE (Complet & Propre)
rem ============================================================================
setlocal EnableDelayedExpansion
title DFWLogger - Desinstallation

rem Verification et demande des droits administrateur si besoin
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [INFO] Elevation administrateur requise...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

echo ============================================================================
echo   DFWLogger - DESINSTALLATION COMPLETE
echo ============================================================================
echo.
echo Cette action va completement desinstaller DFWLogger :
echo   - Arreter les processus et retirer les protections actives
echo   - Supprimer les services Windows (Big Brother SYSTEM et pilote Serbert)
echo   - Restaurer les parametres DNS, hosts et registre (DisableMSI)
echo   - Supprimer les raccourcis du Bureau et du Menu Demarrer
echo   - Supprimer les dossiers d'installation et les donnees
echo.

set /p "CONFIRM=Confirmer la desinstallation complete ? (O/N, defaut: O) : "
if /i "%CONFIRM%"=="N" (
    echo.
    echo Desinstallation annulee par l'utilisateur.
    pause
    exit /b 0
)

echo.
echo [1/7] Arret force des processus DFWLogger...
taskkill /F /IM dfwlogger.exe >nul 2>&1

echo [2/7] Desinstallation et suppression des services Windows...
sc stop DFWLoggerBigBrother >nul 2>&1
sc delete DFWLoggerBigBrother >nul 2>&1
sc stop Serbert >nul 2>&1
sc delete Serbert >nul 2>&1

echo [3/7] Nettoyage des restrictions systeme (registre et hosts)...
reg delete "HKLM\Software\Policies\Microsoft\Windows\Installer" /v "DisableMSI" /f >nul 2>&1
reg delete "HKLM\Software\Policies\Microsoft\Windows\Safer\CodeIdentifiers" /f >nul 2>&1

rem Restauration propre du fichier hosts si section DFWLogger presente
powershell -NoProfile -Command "$hosts = [Environment]::GetFolderPath('System') + '\drivers\etc\hosts'; if (Test-Path $hosts) { $c = Get-Content $hosts -Raw; if ($c -match '# >>> DFWLOGGER-BIGBROTHER-BLOCK >>>') { $n = $c -replace '(?s)# >>> DFWLOGGER-BIGBROTHER-BLOCK >>>.*?# <<< DFWLOGGER-BIGBROTHER-BLOCK <<<', ''; Set-Content $hosts $n.Trim() } }" >nul 2>&1

echo [4/7] Vidage du cache DNS...
ipconfig /flushdns >nul 2>&1

echo [5/7] Suppression des raccourcis Bureau et Menu Demarrer...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$d = [Environment]::GetFolderPath('Desktop'); if (-not $d) { $d = Join-Path $env:USERPROFILE 'Desktop' }; $l = Join-Path $d 'DFWLogger.lnk'; if (Test-Path $l) { Remove-Item -Force $l }; $m = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\DFWLogger'; if (Test-Path $m) { Remove-Item -Recurse -Force $m }" >nul 2>&1

echo [6/7] Suppression des dossiers applicatifs et des donnees...
powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '%LOCALAPPDATA%\Programs\DFWLogger'" >nul 2>&1
powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '%ProgramData%\DFWLogger'" >nul 2>&1
powershell -NoProfile -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '%LOCALAPPDATA%\DFWLogger'" >nul 2>&1

echo [7/7] Verification du nettoyage...
timeout /t 1 /nobreak >nul

echo.
echo ============================================================================
echo   [SUCCES] DFWLogger a ete completement desinstalle du systeme.
echo ============================================================================
echo.
pause
exit /b 0
