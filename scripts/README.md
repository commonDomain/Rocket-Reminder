# Windows Packaging

Run from the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1
```

The script builds the Tauri NSIS installer and copies `Rocket Reminder_0.1.0_x64-setup.exe` to the desktop by default.

Optional parameters:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1 -OutputDirectory "D:\builds"
powershell -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1 -SkipDesktopCopy
```
