# Project Blueprint: Vivaldi JIT Mod Interceptor

> **Maintenance Guideline**: This document is the living technical specification for the project. Whenever a new feature is added, architecture is modified, or a bug is resolved, this document **must** be updated alongside the changes. Keep the Architecture, Component Map, and Changelog synchronized.

---

## 1. Executive Summary

**Vivaldi JIT Mod Interceptor** is a lightweight, high-performance Windows systems utility written in Rust. It guarantees that custom JavaScript and CSS modifications for Vivaldi browser remain persistently loaded across browser updates without running a persistent background daemon.

### Key Value Propositions
- **Zero Background Resource Footprint**: Does not run a persistent background watcher or polling service. Runs synchronously for 2–5 milliseconds only during browser invocation.
- **Update-Proof Mod Injection**: When Vivaldi auto-updates and creates a fresh version folder (wiping custom mod hooks), the interceptor automatically catches the next launch, restores mod files, re-patches `window.html`, and compiles the bundle before the UI paints.
- **Safe Browser Access (Zero Lockout)**: If disk corruption, permission failure, or mod syntax errors occur, a native Win32 dialog allows launching unmodded Vivaldi immediately.

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
                     - Query HKCU/HKLM App Paths
                     - Scan latest semantic version folder
                                  │
                                  ▼
                     [2. Idempotent Patch Verification]
                     - Check window.html for injection hook
                     - If missing: backup to window.html.orig
                     - Atomically inject <script src="inject_bundle.js">
                                  │
                                  ▼
                     [3. Dynamic CSP Bundle Compilation]
                     - Read enabled mods from %APPDATA%
                     - Combine CSS -> inject_bundle.css
                     - Wrap JS mods in isolated IIFEs + try/catch
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
   M:\Vivaldi\Application\vivaldi_real.exe -> M:\Vivaldi\Application\vivaldi.exe
   ```
2. Hardlinks take **<0.01ms** to create and consume **0 bytes** of additional disk storage.
3. The IFEO registry key is only registered for `vivaldi.exe`. Because Windows checks IFEO based on the file name being executed, launching `vivaldi_real.exe` **completely bypasses IFEO**.
4. Standard `CreateProcessW` is used without debugging flags.
5. Child processes (`--type=renderer`, `--type=gpu-process`) inherit the binary name `vivaldi_real.exe` and also run directly without triggering IFEO overhead.

---

### 2.3 Chromium CSP-Compliant Bundling

Vivaldi's UI document (`window.html`) runs under a restricted Chromium extension origin (`chrome-extension://...`). Loading user mods via `file:///C:/Users/...` paths fails due to strict Content Security Policy (CSP) blocking external file schemas.

The interceptor resolves this by generating local bundle files directly within `<version_dir>\resources\vivaldi\`:
- **`inject_bundle.css`**: Aggregates all enabled CSS mods.
- **`inject_bundle.js`**:
  - Dynamically injects `<link rel="stylesheet" href="inject_bundle.css">` into `<head>`.
  - Implements DOM readiness polling (`#browser` root container check).
  - Encapsulates every enabled JS mod in an isolated IIFE with dedicated `try...catch` logging to ensure a faulty mod never crashes the Vivaldi browser window.

---

## 3. Directory Layout & Data Boundaries

```
%APPDATA%\VivaldiModManager\               <-- Safe from browser updates
├── mods_config.json                       <-- Configuration & mod state registry
└── user_mods\
    ├── css\                               <-- Raw CSS mod source files
    └── js\                                <-- Raw JS mod source files

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
| **`main.rs`** | `src/main.rs` | CLI argument routing, IFEO parameter detection, orchestration of JIT checks. |
| **`launcher.rs`** | `src/launcher.rs` | Hardlink synchronization (`ensure_real_executable`), Win32 `CreateProcessW`, command-line quoting, and failsafe `MessageBoxW`. |
| **`patcher.rs`** | `src/patcher.rs` | Idempotent modification of `window.html`, preservation of `.orig` backups, legacy script deduplication, and atomic `.tmp` file renaming. |
| **`bundler.rs`** | `src/bundler.rs` | Aggregates enabled CSS/JS, creates error-trapping IIFEs, generates `#browser` UI readiness poller, mirrors files for legacy mod compatibility. |
| **`discovery.rs`** | `src/discovery.rs` | Registry lookups (HKCU/HKLM App Paths), common installation directory scans, semantic version folder parser/sorter, UNC prefix cleaner. |
| **`config.rs`** | `src/config.rs` | Schema serialization (`ModConfig`, `ModItem`), JSON persistence in `%APPDATA%`, auto-import from existing directories, mod toggling. |
| **`os_hook.rs`** | `src/os_hook.rs` | Reads/writes IFEO `Debugger` key in `HKLM`, automatic UAC elevation via `ShellExecuteExW("runas")`, status reporter. |

---

## 5. CLI Specification

```
USAGE:
    interceptor [SUBCOMMAND]
    interceptor [VIVALDI_ARGUMENTS...]

SUBCOMMANDS:
    launch [ARGS...]      JIT-checks Vivaldi, applies mods if needed, and launches
    setup                 Initial setup: creates directories and auto-imports existing mods
    status [--json]       Display hook status, Vivaldi installation info, and mod stats
    list-mods [--json]    List all registered mods and their enabled/disabled states
    toggle <MOD_NAME>     Toggle a mod between enabled and disabled
    import <FILE_PATH>    Import a new .css or .js mod file into the manager
    patch                 Manually compile bundles and patch window.html
    unpatch               Restore pristine window.html without mod hooks
    install-hook          Register IFEO debugger hook (elevates via UAC if needed)
    uninstall-hook        Remove IFEO debugger hook (elevates via UAC if needed)
    help                  Print this help message
```

---

## 6. Living Maintenance & Change Protocol

Whenever modifications are made to this codebase, developers/agents must adhere to the following protocol:

1. **Keep Hardlinks Intact**: Do **not** revert process creation to direct `vivaldi.exe` with debugger flags. Windows kernel IFEO checks the target image name, which causes recursive loops.
2. **Preserve Idempotency**: All patching logic must check for existence of hooks before editing `window.html`. Never inject twice.
3. **Pristine Backups**: Never overwrite `window.html.orig` if it already exists; it holds the unmodded baseline from the Vivaldi installer.
4. **Maintain Test Coverage**: Run `cargo test` before submitting changes. Ensure unit tests in `tests/patcher_tests.rs` remain isolated using `VIVALDI_MOD_MANAGER_DIR`.
5. **Update Changelog Below**: Log every notable change in Section 7.

---

## 7. Change Log

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
