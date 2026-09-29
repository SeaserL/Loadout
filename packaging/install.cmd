@echo off
rem SPDX-License-Identifier: Apache-2.0
rem Double-click to install or upgrade Loadout for the current user.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
echo.
pause
