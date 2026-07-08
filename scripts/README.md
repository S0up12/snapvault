# Helper scripts

These scripts get SnapVault running on a Windows PC without needing to know
any of the underlying tooling. Run them in this order:

1. **`Install-Dependencies.ps1`** - one-time setup. Installs Node.js, Rust,
   and the C++ Build Tools that Rust needs on Windows. Asks for an
   Administrator prompt once - click "Yes" when Windows asks.
2. **`Check-Environment.ps1`** - confirms everything installed correctly.
   Safe to run anytime; it never changes anything.
3. **`Start-Dev.ps1`** - launches the app. Use this day-to-day while working
   on SnapVault.
4. **`Clean-Build.ps1`** - wipes all build files and rebuilds from scratch.
   Use this if something feels broken, or after pulling new changes that
   don't seem to take effect. Add `-Release` to build the real installer
   (`.\Clean-Build.ps1 -Release`) instead of a quick debug build.

## How to run a script

Open PowerShell **in this `scripts` folder** (in File Explorer: right-click
inside the folder while holding Shift > "Open PowerShell window here"), then
type the script name, for example:

```
.\Install-Dependencies.ps1
```

### "Running scripts is disabled on this system"

Windows blocks running `.ps1` files by default. If you see an error like
that, run this instead (only needed once per script):

```
powershell -ExecutionPolicy Bypass -File .\Install-Dependencies.ps1
```

## What if something goes wrong?

Run `Check-Environment.ps1` first - it will tell you exactly what's missing.
If a build fails, scroll up in the terminal window; the actual error is
usually near the top of the red text, not the very last line.
