# Project Blueprint: Vivaldi JIT Mod Interceptor

> **Maintenance Guideline**: This document is the living technical specification for the project. Whenever a new feature is added, architecture is modified, or a bug is resolved, this document **must** be updated alongside the changes. Keep the Architecture, Component Map, and Changelog synchronized.

---

## 1. Executive Summary

**Vivaldi JIT Mod Interceptor** is a lightweight, high-performance Windows systems utility written in Rust. It guarantees that custom JavaScript and CSS modifications for Vivaldi browser remain persistently loaded across browser updates without running a persistent background daemon.

### Key Value Propositions
- **Zero Background Resource Footprint**: Does not run a persistent background watcher or polling service. Runs synchronously for 2–5 milliseconds only during browser invocation.
- **Update-Proof Mod Injection**: When Vivaldi auto-updates and creates a fresh version folder (wiping custom mod hooks), the interceptor automatically catches the next launch, restores mod files, re-patches `window.html`, and compiles the bundle before the UI paints.
- **Pure Injection Policy**: Leaves all other user scripts, comments, and tags in `window.html` completely untouched. Only inserts its own hook tag.
- **Safe Browser Access (Zero Lockout)**: If disk corruption, permission failure, or mod syntax errors occur, a native Win32 dialog allows launching unmodded Vivaldi immediately.
- **Portable & Multi-Drive Aware**: Dynamically detects all Windows logical drives (A–Z), supports Vivaldi Snapshot, and includes a first-class Portable Mode.

---

## 2. Technical Architecture & Systems Engineering

### 2.1 The JIT Interception Lifecycle

```
[User Action: Desktop Icon / Link / Auto-Update Restart]
                     │
                     ▼
  [Windows Kernel: Checks IFEO for 'vivaldi.exe']
                     │
                     ▼ (Routes to Debugger)
          [interceptor.exe <args...>]
                     │
        ┌────────────┴────────────┐
        ▼                         ▼
 [CLI Subcommand?]        [Launch Request / IFEO Target]
   (setup, status,                │
   toggle, list, etc.)            ▼
                     [1. Resolve Vivaldi Installation]
                     - Query HKCU/HKLM App Paths (Standard & Snapshot)
                     - Scan all logical drives (A-Z) via GetLogicalDrives
                     - Resolve latest semantic version directory
                                  │
                                  ▼
                     [2. Idempotent Pure Patch Verification]
                     - Check window.html for injection hook
                     - If missing: backup to window.html.orig
                     - Atomically inject <script src="inject_bundle.js">
                     - Preserve all other user modifications untouched
                                  │
                                  ▼
                     [3. Dynamic CSP Bundle Compilation]
                     - Read enabled mods from active storage (AppData or Portable)
                     - Sort mods by order priority ascending, then name
                     - Combine CSS -> inject_bundle.css
                     - Wrap JS mods in isolated IIFEs + try/catch
                     - Inject configurable UI readiness selector & delay
                     - Output to <version>\resources\vivaldi\
                                  │
                                  ▼
                     [4. Hardlink Generation (Loop Bypass)]
                     - Create/update NTFS link 'vivaldi_real.exe'
                                  │
                                  ▼
                     [5. Win32 Process Execution]
                     - CreateProcessW("vivaldi_real.exe", <forwarded_args>)
                     - Close handles & terminate interceptor
                                  │
                                  ▼ (~2-5ms total elapsed)
                 [Vivaldi Browser Runs at Full Native Speed]
```

---

### 2.2 Windows IFEO Hooking & The Hardlink Loop-Bypass

#### The IFEO Recursion Trap
Windows Image File Execution Options (`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\<image_name>`) allows registering a `Debugger` string. When any application attempts to execute `vivaldi.exe`, the Windows kernel redirects execution to `interceptor.exe`.

However, if `interceptor.exe` attempts to execute `vivaldi.exe` (even with flags like `DEBUG_ONLY_THIS_PROCESS`), the kernel checks IFEO for the target image name and **re-triggers the debugger**, creating an infinite recursion loop that exhausts system handles.

#### The NTFS Hardlink Solution
To bypass IFEO with zero latency and zero disk penalty:
1. In the Vivaldi Application directory, the interceptor creates an NTFS hardlink:
   ```text
   <VivaldiDir>\Application\vivaldi_real.exe -> <VivaldiDir>\Application\vivaldi.exe
   ```
2. Hardlinks take **<0.01ms** to create and consume **0 bytes** of additional disk storage.
3. The IFEO registry key is only registered for `vivaldi.exe`. Because Windows checks IFEO based on the file name being executed, launching `vivaldi_real.exe` **completely bypasses IFEO**.
4. Standard `CreateProcessW` is used without debugging flags.
5. Child processes (`--type=renderer`, `--type=gpu-process`) inherit the binary name `vivaldi_real.exe` and also run directly without triggering IFEO overhead.

---

### 2.3 Dynamic Drive Discovery & Snapshot Support

Instead of hardcoding drive letters, the interceptor queries all active logical drives via the Win32 API `GetLogicalDrives()`. For every active volume (`C:`, `D:`, `E:`, `M:`, etc.), it checks:
- Standard Vivaldi paths (`<Drive>:\Vivaldi\Application\vivaldi.exe`)
- Vivaldi Snapshot paths (`<Drive>:\Vivaldi Snapshot\Application\vivaldi.exe`)
- Program Files and PortableApps directories
- Registry `App Paths` for both stable and snapshot builds

---

### 2.4 Chromium CSP-Compliant Bundling & Configurable Readiness

Vivaldi's UI document (`window.html`) runs under a restricted Chromium extension origin (`chrome-extension://...`). Loading user mods via `file:///` paths fails due to strict Content Security Policy (CSP).

The interceptor resolves this by generating local bundle files directly within `<version_dir>\resources\vivaldi\`:
- **`inject_bundle.css`**: Aggregates all enabled CSS mods in ascending order priority.
- **`inject_bundle.js`**:
  - Dynamically injects `<link rel="stylesheet" href="inject_bundle.css">` into `<head>`.
  - Implements DOM readiness polling using the configurable selector (`config.ui_ready_selector`, default `"#browser"`).
  - Enforces configurable initialization delay (`config.init_delay_ms`, default `60ms`).
  - Encapsulates every enabled JS mod in an isolated IIFE with dedicated `try...catch` logging to ensure a faulty mod never crashes the Vivaldi browser window.

---

## 3. Directory Layout & Storage Modes

The interceptor supports two storage modes: **Standard Mode** (default) and **Portable Mode**.

### 3.1 Standard Mode (%APPDATA%)
```
%APPDATA%\VivaldiModManager\               <-- Safe from browser updates
├── mods_config.json                       <-- Configuration & mod state registry
└── user_mods\
    ├── css\                               <-- Raw CSS mod source files
    └── js\                                <-- Raw JS mod source files
```

### 3.2 Portable Mode (Next to Binary)
If `portable.lock`, `mods_config.json`, or a `user_mods/` folder exists directly in the directory of `interceptor.exe`, the application runs in self-contained Portable Mode:
```
<PortableDir>\
├── interceptor.exe
├── portable.lock                          <-- Marker file (optional, created via --portable)
├── mods_config.json                       <-- Portable configuration
└── user_mods\
    ├── css\
    └── js\
```

### 3.3 Target Browser Application Tree
```
<VivaldiDir>\Application\
├── vivaldi.exe                            <-- Intercepted target (IFEO)
├── vivaldi_real.exe                       <-- NTFS Hardlink (unintercepted execution)
└── <version>\resources\vivaldi\
    ├── window.html                        <-- Patched with hook script tag
    ├── window.html.orig                   <-- Pristine original backup
    ├── inject_bundle.css                  <-- Compiled CSS bundle
    ├── inject_bundle.js                   <-- Compiled JS bootstrap bundle
    └── user_mods\                         <-- Mirrored mods directory
```

---

## 4. Module Map & Responsibilities

| Module | Location | Core Responsibilities |
| :--- | :--- | :--- |
| **`main.rs`** | `src/main.rs` | CLI argument routing, interactive path selection menu, IFEO detection, JIT orchestration. |
| **`launcher.rs`** | `src/launcher.rs` | Hardlink synchronization (`ensure_real_executable`), Win32 `CreateProcessW`, command-line quoting, and failsafe `MessageBoxW`. |
| **`patcher.rs`** | `src/patcher.rs` | Pure idempotent modification of `window.html` (leaves other user scripts intact), preservation of `.orig` backups, and atomic `.tmp` file renaming. |
| **`bundler.rs`** | `src/bundler.rs` | Aggregates enabled CSS/JS, sorts by priority order, generates configurable DOM readiness poller, creates error-trapping IIFEs. |
| **`discovery.rs`** | `src/discovery.rs` | Dynamic drive enumeration (`GetLogicalDrives`), registry lookups, snapshot detection, semantic version parser, UNC prefix cleaner. |
| **`config.rs`** | `src/config.rs` | Schema serialization (`ModConfig`, `ModItem`), portable mode detection (`is_portable`), JSON persistence, mod toggling. |
| **`os_hook.rs`** | `src/os_hook.rs` | Reads/writes IFEO `Debugger` key in `HKLM` for custom target exe names, automatic UAC elevation via `ShellExecuteExW("runas")`. |

---

## 5. Configuration Schema (`mods_config.json`)

```json
{
  "version": 1,
  "vivaldi_path": "M:\\Vivaldi\\Application\\vivaldi.exe",
  "auto_patch": true,
  "ui_ready_selector": "#browser",
  "init_delay_ms": 60,
  "detach_timeout_ms": 5000,
  "exe_name": "vivaldi.exe",
  "mods": [
    {
      "name": "BetterAnimation.css",
      "mod_type": "css",
      "enabled": true,
      "order": 0,
      "description": ""
    }
  ]
}
```

---

## 6. CLI Specification

```
USAGE:
    interceptor [SUBCOMMAND] [OPTIONS]
    interceptor [VIVALDI_ARGUMENTS...]

SUBCOMMANDS:
    launch [ARGS...]      JIT-checks Vivaldi, applies mods if needed, and launches
    setup [OPTIONS]       Initial setup: interactive path selection and mod auto-import
                          Options:
                            --portable           Enable portable mode in executable directory
                            --path <PATH>        Specify vivaldi.exe or parent directory directly
                            --select-path        Force interactive selection menu
    status [--json]       Display hook status, storage mode, Vivaldi installation info, and mod stats
    list-mods [--json]    List all registered mods, load order, and their enabled/disabled states
    toggle <MOD_NAME>     Toggle a mod between enabled and disabled
    import <FILE_PATH>    Import a new .css or .js mod file into the manager
    patch                 Manually compile bundles and patch window.html
    unpatch               Restore pristine window.html without mod hooks
    install-hook [OPTS]   Register IFEO debugger hook (elevates via UAC if needed)
                          Options:
                            --path <PATH>        Specify target vivaldi.exe or directory
                            --target <EXE_NAME>  Specify target exe name (default: vivaldi.exe)
    uninstall-hook [OPTS] Remove IFEO debugger hook (elevates via UAC if needed)
                          Options:
                            --target <EXE_NAME>  Specify target exe name (default: vivaldi.exe)
    help                  Print this help message
```

---

## 7. Living Maintenance & Change Protocol

Whenever modifications are made to this codebase, developers/agents must adhere to the following protocol:

1. **Keep Hardlinks Intact**: Do **not** revert process creation to direct `vivaldi.exe` with debugger flags. Windows kernel IFEO checks the target image name, which causes recursive loops.
2. **Pure Injections Only**: Never delete or comment out other scripts in `window.html`. Only inject/remove the interceptor's own hook tag.
3. **Preserve Idempotency**: All patching logic must check for existence of hooks before editing `window.html`. Never inject twice.
4. **Pristine Backups**: Never overwrite `window.html.orig` if it already exists; it holds the unmodded baseline from the Vivaldi installer.
5. **Maintain Test Coverage**: Run `cargo test` before submitting changes. Ensure unit tests in `tests/patcher_tests.rs` remain isolated using `VIVALDI_MOD_MANAGER_DIR`.
6. **Update Changelog Below**: Log every notable change in Section 8.

---

## 8. Change Log

### [v0.1.2] - 2026-09-15
- **Feature**: Replaced hardcoded drive letters (`[M, D, E, C]`) with dynamic logical drive scanning via `GetLogicalDrives()`, detecting all active drives (A–Z) across the system.
- **Feature**: Added interactive Vivaldi installation selection menu with guidelines, discovered candidate lists, and support for manual custom path input.
- **Feature**: Added first-class **Portable Storage Mode** (`--portable` flag and automatic detection of local `portable.lock`/`mods_config.json`).
- **Feature**: Added configurable DOM readiness selector (`ui_ready_selector`), initialization delay (`init_delay_ms`), detach timeout (`detach_timeout_ms`), and custom executable name (`exe_name`) in `mods_config.json`.
- **Feature**: Added explicit mod load prioritization (`order` field in `ModItem`).
- **Policy**: Enforced **Pure Injection Policy** in `patcher.rs`, completely removing assumptions/comments regarding legacy user mods (`injectMods.js`) and keeping all existing HTML untouched.

### [v0.1.1] - 2026-09-15
- **Fix (Critical)**: Resolved IFEO infinite recursion loop by transitioning from `DEBUG_ONLY_THIS_PROCESS` to the **NTFS Hardlink Bypass** pattern (`vivaldi_real.exe`).
- **Fix**: Removed verbatim `\\?\` UNC path prefixes returned by Rust's `canonicalize()` to ensure compatibility with Chromium's internal path parser.
- **Improvement**: Added test environment sandbox isolation using `VIVALDI_MOD_MANAGER_DIR` so unit tests do not touch user AppData.
- **Documentation**: Created technical blueprint (`PROJECT.md`) and user walkthrough.

### [v0.1.0] - 2026-09-15
- **Initial Release**: Complete Rust JIT interceptor CLI.
- **Features**:
  - IFEO registry integration with automatic UAC elevation.
  - Multi-tier Vivaldi detection (HKCU/HKLM registry, filesystem scan).
  - Idempotent `window.html` patcher with atomic write and `.orig` backup.
  - CSP-compliant bundler compiling into `inject_bundle.css` and `inject_bundle.js`.
  - Per-mod isolated IIFE wrappers and DOM readiness poller.
  - Native Win32 `MessageBoxW` failsafe dialog.
  - Full CLI management suite (`setup`, `toggle`, `import`, `list-mods`, `status`).
