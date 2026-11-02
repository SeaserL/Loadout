@echo off
rem SPDX-License-Identifier: Apache-2.0
"%~dp0node.exe" "%~dp0tools\desktop.mjs" stop %*
if errorlevel 1 pause
