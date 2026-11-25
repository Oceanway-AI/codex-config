@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-desktop.ps1"
set "owExit=%errorlevel%"
echo.
pause
exit /b %owExit%
