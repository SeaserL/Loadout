Loadout - All your AI, one endpoint.
=====================================

Install / upgrade
-----------------
Double-click install.cmd (or run: powershell -ExecutionPolicy Bypass -File install.ps1).
Options: -AddToPath (use `loadout` in terminals), -Autostart (start at sign-in), -NoStart.

Programs:  %LOCALAPPDATA%\Programs\Loadout
Your data: %LOCALAPPDATA%\Loadout  (API keys live in Windows Credential Manager)

What is inside
--------------
loadout-app.exe   Desktop app (tray icon). Closing the window keeps the gateway running.
loadoutd.exe      The gateway (background process, no window).
loadout.exe       Command-line tool.

First steps
-----------
1. Open "Loadout" from the Start Menu.
2. Accounts & usage -> Add account (API key) or Scan this PC.
3. Client keys -> New client key. Point your app at http://127.0.0.1:8317/v1 and use
   the key as its OpenAI or Anthropic API key.
4. Model names: loadout/default, loadout/fast, loadout/realtime, a pool, or an exact
   provider/account/model from the Models page.

Uninstall
---------
powershell -ExecutionPolicy Bypass -File "%LOCALAPPDATA%\Programs\Loadout\uninstall.ps1"
Add -RemoveData to delete your data and stored API keys as well.

License: Apache-2.0 (LICENSE). Third-party licenses: THIRD_PARTY_LICENSES.md.

Private repositories: the updater reads your existing default Git Credential Manager
GitHub login from Windows Credential Manager, without prompting or storing new keys.
That account must have repository read access. Missing/expired login is shown as an
update error; sign in with Git for Windows and check again. Main program updates do
not replace separately installed OpenCode/WorkBuddy bridge directories.
