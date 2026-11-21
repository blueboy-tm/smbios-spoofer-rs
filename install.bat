@echo off
setlocal EnableExtensions EnableDelayedExpansion
title SMBIOS Spoofer - Installer and Driver Manager
cd /d "%~dp0"

:: Pass all command-line parameters unchanged to the PowerShell installer.
if not "%~1"=="" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
    exit /b !ERRORLEVEL!
)

echo.
echo   ================================================================
echo       SMBIOS SPOOFER - INSTALL / STATUS / SAFE DRIVER CLEANUP
echo   ================================================================
echo.
echo     [1] Complete install: build UEFI + driver, sign, configure next boot
echo     [2] Status: serials, service image and driver file inventory
echo     [3] Build only: tests, UEFI and patched Windows driver
echo     [4] Install driver only: build, sign and configure next boot
echo     [5] Audit project driver files (read-only)
echo     [6] Cleanup project test drivers (run again after reboot)
echo     [7] Uninstall driver only (preserve UEFI profile and boot entry)
echo     [8] Reset spoof profile (generates new identity on next boot)
echo     [9] Full uninstall: remove service and UEFI boot entry
echo     [10] Complete install with MAC spoofing
echo     [11] Complete headless install with MAC spoofing
echo.
set /p CHOICE="  Select [1-11, Enter=1]: "
if "%CHOICE%"=="" set "CHOICE=1"

set "MODE="
set "EXTRA="
if "%CHOICE%"=="1" set "MODE=install"
if "%CHOICE%"=="2" set "MODE=status"
if "%CHOICE%"=="3" set "MODE=build-only"
if "%CHOICE%"=="4" set "MODE=install-driver"
if "%CHOICE%"=="5" set "MODE=audit-drivers"
if "%CHOICE%"=="6" set "MODE=cleanup-drivers"
if "%CHOICE%"=="7" set "MODE=uninstall-driver"
if "%CHOICE%"=="8" set "MODE=reset-profile"
if "%CHOICE%"=="9" set "MODE=uninstall"
if "%CHOICE%"=="10" (
    set "MODE=install"
    set "EXTRA=-SpoofMac"
)
if "%CHOICE%"=="11" (
    set "MODE=install"
    set "EXTRA=-Headless -SpoofMac"
)

if not defined MODE (
    echo   [!] Invalid selection.
    exit /b 2
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" -Mode !MODE! !EXTRA!
set "RESULT=!ERRORLEVEL!"
echo.
if not "!RESULT!"=="0" echo   [!] Installer exited with code !RESULT!.
pause
exit /b !RESULT!
