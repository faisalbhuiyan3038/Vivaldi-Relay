# Vivaldi JIT Mod Interceptor — UX Problem Analysis

> A full audit of user-facing problems discovered by simulating a fresh user experience.
> Each issue describes what the user sees, why it happens in code, and what the correct behavior should be.

---

## Problem 1 — `patch` (and most commands) silently succeed without any setup

### What the user sees
```
> interceptor patch
✓ Compiled bundle (0 CSS, 0 JS)
✓ window.html is already patched.
```
No error. No warning. Succeeds. Appears healthy. But 0 mods were compiled, and it only "worked" because it found Vivaldi through auto-discovery without the user ever doing setup.

### Root cause
`run_patch_command()` calls `ModConfig::load().unwrap_or_default()`. When no `mods_config.json` exists, it returns a completely default config with:
- `vivaldi_path = None`
- `mods = []`

Then `discovery::resolve_vivaldi(None)` is called. Because `vivaldi_path` is `None`, it falls through to `discover_all_installations()`, which scans the registry and all drives A–Z and **silently finds and uses** the first Vivaldi it discovers — no user confirmation needed.

The bundle compiles cleanly but with 0 mods because no mods are registered in the empty default config. The patch succeeds because `window.html` was already patched from a previous session.

### What should happen
If no `mods_config.json` exists (i.e., user has never run `setup`), every command that requires a configured state should:
1. Print a clear message: `"Setup has not been run. Please run 'interceptor setup' first."`
2. Exit with a non-zero code.

Or at minimum, `patch` should warn: `"No mods are registered. Running setup first is recommended."` and show how many files are sitting in the user_mods directories.

---

## Problem 2 — Vivaldi is auto-discovered silently from the wrong source

### What the user sees
The user deleted `mods_config.json` to start fresh, but commands still show:
```
Vivaldi Executable: M:\Vivaldi\Application\vivaldi.exe
```
The user has no idea where this came from, and cannot control it.

### Root cause
All commands that call `discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new))`:
- When `config.vivaldi_path` is `None`, the hint is `None`
- `resolve_vivaldi(None)` calls `discover_all_installations()` automatically
- This hits the Windows registry (`HKCU/HKLM App Paths`) and finds Vivaldi instantly

The Vivaldi path is discovered silently with no output and no confirmation prompt.

### What should happen
Auto-discovery is a useful feature, but the user must be told:
- That discovery happened
- What was discovered
- And given a chance to confirm or change it

Something like:
```
No Vivaldi path configured. Auto-discovered: M:\Vivaldi\Application\vivaldi.exe
Using this path. Run 'interceptor setup' to confirm or change.
```

Even better: for commands that modify files (like `patch`), require explicit configuration. For read-only commands (like `status`), auto-discovery with a notice is acceptable.

---

## Problem 3 — `patch` does not prompt for path when unconfigured

### What the user sees
```
> interceptor patch
✓ Compiled bundle (0 CSS, 0 JS)
✓ window.html is already patched.
```
It silently used auto-discovered Vivaldi and did not ask the user where Vivaldi is.

### Root cause
`run_patch_command()` has no path selection logic at all. It just calls `ModConfig::load().unwrap_or_default()` and passes the path (or lack thereof) directly to `resolve_vivaldi`. It was designed as a "manual trigger" subcommand, not a first-time-setup command.

### What should happen
`patch` should either:
- **Option A**: Call `select_or_confirm_vivaldi_path()` when `config.vivaldi_path` is `None`, same as `setup` does.
- **Option B**: Reject the operation with a clear message: `"Vivaldi path not configured. Run 'interceptor setup' first."` — this is the simpler and more correct approach, since `patch` should not be a first-run command.

---

## Problem 4 — `mods_config.json` is never created until `setup` is explicitly run, but many commands silently continue without it

### What the user sees
After deleting the config folder, commands like `status`, `patch`, `list-mods`, `install-hook`, and `uninstall-hook` all continue to work silently using `unwrap_or_default()`. The user sees no indication that anything is wrong.

### Root cause
`ModConfig::load()` returns `Ok(Self::default())` when the config file doesn't exist. This silent fallback is an intentional design for the JIT launch path (so Vivaldi can launch even without config), but it's being applied equally to all CLI management commands where it causes confusion.

### What should happen
JIT launch path should use silent fallback (to not block Vivaldi from opening).

CLI management commands (`setup`, `patch`, `status`, `list-mods`, `toggle`, `install-hook`) should distinguish between:
- "Config file doesn't exist" → Show a guided message
- "Config file exists but is malformed" → Show a parse error

---

## Problem 5 — `list-mods` silently rescans and creates entries without telling the user

### What the user sees
After deleting `mods_config.json`, running `list-mods` shows all 36 mods as Enabled:
```
=== Registered Mods ===
1    0      CSS    BetterAnimation.css    [Enabled]
...
36   0      JS     VividToast.js          [Enabled]
```
This appears to show healthy state, but is in fact derived entirely from scanning the filesystem — no `mods_config.json` exists.

### Root cause
`run_list_mods()` calls `config.rescan_mods()` unconditionally. `rescan_mods()` scans the `user_mods/css/` and `user_mods/js/` directories and populates the mods list from disk. Even with no saved config, all files present in those directories get shown as Enabled. It also calls `self.save()` internally, which silently creates a new `mods_config.json`.

The user deleted their config to start fresh but `list-mods` recreated it silently.

### What should happen
`list-mods` should not mutate state by default. Options:
- Show a notice: `"mods_config.json not found. Showing filesystem scan. Run 'setup' to save configuration."`
- Remove the `rescan_mods()` call from `list-mods` entirely and only show what is in the saved config
- Add a `--rescan` flag to `list-mods` for explicit rescanning

---

## Problem 6 — `unpatch` does not clean up `window.html.orig`

### What the user sees
After running `interceptor unpatch`, the file `window.html.orig` remains in the Vivaldi resources directory:
```
M:\Vivaldi\Application\8.2.4133.52\resources\vivaldi\window.html
M:\Vivaldi\Application\8.2.4133.52\resources\vivaldi\window.html.orig  ← still there
```

### Root cause
`run_unpatch_command()` calls `patcher::unpatch_window_html()` which restores `window.html` from `window.html.orig` but deliberately leaves the `.orig` file in place (it's designed as a safety backup).

### What should happen
Two options:
- **Recommended**: Keep `window.html.orig` in place and tell the user: `"Original backup preserved at window.html.orig"`. This is safer.
- **Alternative**: Offer a `--clean` flag: `interceptor unpatch --clean` deletes the `.orig` file too.

Either way, the user should be told what was left behind and why, so they understand the state of their installation.

---

## Problem 7 — `setup` says it imported 71 mods but `patch` compiled 0

### What the user sees
Running `setup`:
```
✓ Auto-imported/registered 71 mods into configuration
✓ Initial JIT patch & bundle compiled successfully!
```
Then running `patch` independently later:
```
✓ Compiled bundle (0 CSS, 0 JS)
```

### Root cause
The "71 mods imported" count inflates due to triple-counting: `auto_import_from_dir()` runs on two potential source directories, then `rescan_mods()` runs again on `user_mods/`. In the scenario where `user_mods/` already contained 36 files and the other directories were empty, the count might still be inflated by directory-traversal overlap. However, the real issue is that when `patch` runs later without mods in config (because config was deleted), it compiles 0 mods silently.

The imported count being `71` for `36` actual mods files is suspicious and should be investigated (likely double-importing from subdirectories).

### What should happen
- The import count should accurately reflect unique newly-registered mods
- `patch` should warn if 0 mods will be included: `"Warning: No mods are enabled. The bundle will be empty."`

---

## Problem 8 — Version string displayed in `version` command is out of date

### What the user sees
```
> interceptor version
Vivaldi JIT Mod Interceptor v0.1.2
```
But the actual built binary is v0.1.3.

### Root cause
The version string in `main.rs` line 86 was not updated when the changelog was bumped to v0.1.3.

### What should happen
The version string in `main.rs` should match `Cargo.toml` and `PROJECT.md`. It should be derived from `Cargo.toml` via the `env!("CARGO_PKG_VERSION")` macro to prevent this drifting again:
```rust
println!("Vivaldi JIT Mod Interceptor v{}", env!("CARGO_PKG_VERSION"));
```

---

## Problem 9 — No clear "first run" experience or onboarding guidance

### What the user sees
A new user downloads `interceptor.exe`, opens a terminal, and types:
```
> interceptor
```
Nothing visible happens (the JIT launch mode fires, finds Vivaldi via auto-discovery, tries to compile 0 mods, and exits).

### Root cause
When called with no arguments, `run_jit_launch(None, &[])` is invoked. This is correct for the IFEO hook use-case, but for a human running the binary for the first time, it gives no guidance at all.

### What should happen
If called with no arguments AND no `mods_config.json` exists, show an onboarding message:
```
Vivaldi JIT Mod Interceptor v0.1.3

First-time setup required. Run:
  interceptor setup          — to configure your Vivaldi path and mod directories
  interceptor help           — to see all available commands
```

---

## Problem 10 — `install-hook` can be called before `setup`, causing a partially broken state

### What the user sees
A new user reads the README and runs `install-hook` directly without running `setup` first. Vivaldi is now intercepted, but no mods exist in config, so every launch compiles an empty bundle and patches `window.html` with nothing useful.

### Root cause
There is no precondition check in `run_install_hook()` that verifies whether `setup` has been run. The IFEO hook is installed, but the user's mods are not registered.

### What should happen
`install-hook` should check if `mods_config.json` exists with a non-empty mods list. If not, warn the user:
```
Warning: No mods are registered yet.
Run 'interceptor setup' first to import your mods before installing the hook.
Continue anyway? [y/N]
```

---

## Summary Table

| # | Command | Problem | Severity |
|---|---------|---------|----------|
| 1 | `patch` | Silently succeeds with 0 mods and no config | 🔴 High |
| 2 | All commands | Vivaldi auto-discovered silently with no notification | 🔴 High |
| 3 | `patch` | No path prompt when Vivaldi path is unconfigured | 🔴 High |
| 4 | All management commands | `unwrap_or_default()` masks missing config state | 🟠 Medium |
| 5 | `list-mods` | Silently rescans filesystem and recreates config | 🟠 Medium |
| 6 | `unpatch` | Leaves `window.html.orig` behind without informing user | 🟡 Low |
| 7 | `setup` / `patch` | Auto-import count inflated, patch later compiles 0 | 🟠 Medium |
| 8 | `version` | Version string is v0.1.2, binary is v0.1.3 | 🟡 Low |
| 9 | (no args) | No onboarding when binary is run for the first time | 🟠 Medium |
| 10 | `install-hook` | No warning when called before `setup` | 🟠 Medium |

---

## Recommended Fix Priority

1. **Add a "setup check" gate** to all non-JIT commands: if no `mods_config.json` exists, block and show a setup prompt.
2. **Fix `patch` to prompt for Vivaldi path** when `vivaldi_path` is `None` in config.
3. **Emit a notice whenever auto-discovery is used** in place of a configured path.
4. **Fix `list-mods`** to not mutate state during a read-only listing operation.
5. **Fix version string** to use `env!("CARGO_PKG_VERSION")`.
6. **Add onboarding message** when run with no arguments and no config.
7. **Add a precondition warning** to `install-hook` when no mods are registered.
