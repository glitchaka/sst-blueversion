@echo off
setlocal
cd /d "%~dp0"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-sst.ps1"
set "ERR=%ERRORLEVEL%"

if not "%ERR%"=="0" (
  echo.
  echo ERROR: build-sst fallo con codigo %ERR%.
  pause
  exit /b %ERR%
)

echo.
pause
exit /b 0
