# Vivaldi one-shot mod patcher (Python)

A single stdlib-only script (Python 3.8+) that patches Vivaldi with your CSS/JS mods **once, when you run it**. It installs no hook and runs nothing in the background, so other apps launching Vivaldi are unaffected. After a Vivaldi update, run it again.

## Setup

1. Edit the config block at the top of `vivaldi_patcher.py`:
   ```python
   VIVALDI_EXE = r"C:\Users\you\AppData\Local\Vivaldi\Application\vivaldi.exe"
   MODS_DIR = None      # None = folder next to the script
   STRETCH_TABS = False # built-in: tabs stretch to fill the tab bar
   ```
2. Put mods in `css/` and `js/` inside `MODS_DIR` (the script creates these folders if they are missing).

## Usage

```
python vivaldi_patcher.py            interactive menu
python vivaldi_patcher.py patch      apply mods (re-run any time to rebuild)
python vivaldi_patcher.py unpatch    revert to stock
python vivaldi_patcher.py order      reorder / enable / disable mods
python vivaldi_patcher.py status     show patch state per version folder
```

Options: `--vivaldi PATH`, `--mods PATH`, `--version 7.5.2.3`, `--stretch-tabs` / `--no-stretch-tabs`.

If more than one version folder exists (Vivaldi keeps the old and new folders side by side while an update is pending), the script asks which one to patch or unpatch. `--version` skips the prompt.

If Vivaldi is installed under Program Files, run the script from an Administrator terminal.

## Load order

Each mods folder has an `order.txt` that the script maintains:

```
BaseTheme.css
Tabs.css
# Experimental.css
```

- The top line loads first. CSS and JS are ordered separately.
- A line starting with `#` is a disabled mod.
- New files are appended automatically. Entries for deleted files are dropped.

You can edit `order.txt` by hand or use `python vivaldi_patcher.py order` (`m 3 1` moves mod 3 to position 1, `t 2` toggles mod 2, `s` saves). Then run `patch` again to apply the new order.

## What it changes

In `<Application>\<version>\resources\vivaldi\`:

| File | Change | Reverted by `unpatch` |
| --- | --- | --- |
| `window.html` | one `<script src="vivaldi-mods.js">` line between `vivaldi-patcher` markers, with a backup in `window.html.vmp-orig` | marker block removed, backup deleted |
| `vivaldi-mods.css` / `vivaldi-mods.js` | compiled bundles (each JS mod runs in its own try/catch) | deleted |
| `bundle.js` (only with stretch tabs) | `=180` → `=5200` max tab width, with a backup in `bundle.js.vmp-orig` | restored from backup |

## Migrating from the Rust interceptor

The old IFEO hook is what interfered with other launchers and re-patched new installs. Remove it once:

```
interceptor uninstall-hook
interceptor unpatch
```

`status`, `patch` and the menu warn if the hook or `vivaldi_real.exe` is still present.
