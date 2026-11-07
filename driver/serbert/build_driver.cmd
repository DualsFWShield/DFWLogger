@echo off
setlocal enabledelayedexpansion
title Compilation du pilote noyau Serbert (WDM x64)

echo =======================================================
echo    DFWLogger - Compilation du pilote noyau Serbert
echo =======================================================
echo.

rem Recherche de vswhere
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
    echo [ERREUR] Visual Studio Installer introuvable (%VSWHERE%^).
    echo Veuillez installer Visual Studio 2022 ou Build Tools avec le WDK ^(Windows Driver Kit^).
    pause
    exit /b 1
)

rem Recherche du chemin d'installation de Visual Studio
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -requires Microsoft.Component.MSBuild -property installationPath`) do (
    set "VSINSTALLDIR=%%i"
)

if not exist "%VSINSTALLDIR%\MSBuild\Current\Bin\MSBuild.exe" (
    echo [ERREUR] MSBuild introuvable dans %VSINSTALLDIR%.
    pause
    exit /b 1
)

set "MSBUILD=%VSINSTALLDIR%\MSBuild\Current\Bin\MSBuild.exe"
echo [OK] MSBuild detecte : %MSBUILD%

echo.
echo Compilation du driver serbert.sys (x64 Release)...
"%MSBUILD%" "%~dp0serbert.vcxproj" /p:Configuration=Release /p:Platform=x64 /t:Rebuild

if %errorlevel% neq 0 (
    echo.
    echo [ERREUR] La compilation du driver a echoue.
    echo Verifiez que le WDK (Windows Driver Kit) est installe et associe a votre version de Visual Studio.
    pause
    exit /b %errorlevel%
)

echo.
echo =======================================================
echo [SUCCES] serbert.sys compile avec succes !
echo =======================================================
pause
exit /b 0

