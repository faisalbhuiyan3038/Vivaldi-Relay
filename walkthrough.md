# Walkthrough: Vivaldi Just-In-Time (JIT) Launch Interceptor

We have built, verified, and benchmarked the Just-In-Time (JIT) Launch Interceptor for Vivaldi browser on Windows.

---

## What Was Created

### 1. Architecture & Core Components

| Component | Source File | Description |
| :--- | :--- | :--- |
| **CLI & Routing** | [main.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/main.rs) | Handles IFEO detection, argument passthrough, and CLI management subcommands (`setup`, `launch`, `status`, `list-mods`, `toggle`, `import`, `patch`, `unpatch`, `install-hook`, `uninstall-hook`). |
| **Mod & Config Manager** | [config.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/config.rs) | Manages `%APPDATA%\VivaldiModManager\mods_config.json` and permanent mod storage in `user_mods/{css,js}`. Supports auto-importing, rescanning, and state toggling. |
| **Vivaldi Discovery** | [discovery.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/discovery.rs) | Multi-tier detection (HKCU/HKLM registry, custom drives, app paths) and semantic version folder resolver. Automatically discovered `M:\Vivaldi\Application` with version `8.2.4133.52`. |
| **CSP Bundle Compiler** | [bundler.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/bundler.rs) | Compiles enabled CSS and JS mods into `inject_bundle.css` and `inject_bundle.js`. Wraps each JS mod in an isolated IIFE with `try...catch` and implements DOM readiness polling (`#browser` root check). |
| **Idempotent Patcher** | [patcher.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/patcher.rs) | Atomically patches `window.html`, creates `window.html.orig` pristine backups, and comments out legacy duplicate scripts. Guarantees no double-injections. |
| **Win32 Detached Launcher** | [launcher.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/launcher.rs) | Uses Win32 `DEBUG_ONLY_THIS_PROCESS` + immediate `DebugActiveProcessStop` to bypass Windows IFEO debugger recursion, with Win32 `MessageBoxW` fallback on failure. |
| **OS IFEO Hook** | [os_hook.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/src/os_hook.rs) | Configures Windows Image File Execution Options (`HKLM\...\Image File Execution Options\vivaldi.exe`) with built-in UAC elevation via `ShellExecuteExW("runas")`. |
| **Unit Test Suite** | [patcher_tests.rs](file:///m:/.systemfile/vivaldi-jit-mod-interceptor/tests/patcher_tests.rs) | Automated test suite verifying idempotency, backup preservation, unpatching, config serialization, and bundle compilation. |

---

## Verification & Test Results

### 1. Automated Test Suite (`cargo test`)
All 4 unit tests executed and passed in 0.01s:
```
running 4 tests
test test_config_serialization_and_toggle ... ok
test test_html_patch_without_body_tag ... ok
test test_html_patch_idempotency ... ok
test test_bundler_compilation ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 2. Standalone Release Build (`cargo build --release`)
- Binary location: `target\release\interceptor.exe`
- Size: ~1.6 MB
- Runtime dependencies: None (pure standalone Win32 binary)

### 3. Vivaldi Auto-Discovery & Mod Import (`interceptor setup`)
Successfully scanned `M:\Vivaldi\Application\8.2.4133.52\resources\vivaldi\user_mods` and imported all existing user mods into `%APPDATA%\VivaldiModManager`:
- Total mods imported: **35 mods** (17 CSS, 18 JS)
- Verified active bundle generation:
  - `inject_bundle.css` (90,189 bytes)
  - `inject_bundle.js` (1,324,956 bytes)

### 4. CLI Subcommand Testing
- `interceptor status --json`: Verified accurate JSON status reporting hook state, Vivaldi path, version, and mod counts.
- `interceptor list-mods`: Verified table formatted list of all 35 CSS and JS mods with enabled statuses.
- `interceptor toggle BetterAnimation.css`: Verified mod disabled, bundle dynamically recompiled, then re-enabled.
- `interceptor import <file>`: Verified importing `.css` and `.js` files from any location into AppData with auto-registration.
- `interceptor launch --version`: Verified spawning Vivaldi with `DEBUG_ONLY_THIS_PROCESS` and detaching cleanly within single-digit milliseconds.

---

## How to Activate Full Automatic Launch Interception

To route all Vivaldi launches (desktop shortcuts, external links, taskbar, browser auto-restarts) through the interceptor:

1. Open a terminal or run:
   ```powershell
   .\target\release\interceptor.exe install-hook
   ```
2. Windows will display the standard UAC elevation prompt. Click **Yes**.
3. Run `.\target\release\interceptor.exe status` to confirm:
   ```
   IFEO Hook: [ACTIVE] Correctly routes vivaldi.exe through this interceptor.
   ```
4. To remove at any time:
   ```powershell
   .\target\release\interceptor.exe uninstall-hook
   ```
