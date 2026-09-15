# Vivaldi Just-In-Time (JIT) Mod Interceptor

A blazingly fast Windows Just-In-Time (JIT) launch interceptor written in Rust that guarantees Vivaldi browser always launches with your custom CSS and JS mods applied—even immediately after automated or internal browser updates.

## Key Features

- **Instant JIT Patching (<5ms Overhead)**: Runs at execution time immediately before browser launch. Checks `window.html` and patches if missing.
- **Windows IFEO Infinite-Loop Prevention**: Uses Win32 debugging APIs (`DEBUG_ONLY_THIS_PROCESS` + immediate detach) to allow `interceptor.exe` to spawn `vivaldi.exe` without triggering Windows Image File Execution Options recursively.
- **Chromium CSP-Compliant Bundling**: Concatenates enabled CSS and JS mods into `inject_bundle.css` and `inject_bundle.js` directly within Vivaldi's `resources\vivaldi\` package directory, avoiding Chromium extension Content Security Policy (CSP) errors.
- **Mod Isolation & Error Trapping**: Each JavaScript mod is executed in an isolated IIFE with dedicated `try ... catch` blocks so a syntax error or exception in one mod cannot break other mods or the Vivaldi UI.
- **UI Readiness Polling**: Automatically defers DOM-dependent scripts until Vivaldi's root `#browser` container is mounted.
- **Native Failsafe (Zero Lockout Risk)**: If patching ever fails (e.g. disk or permission error), displays a native Win32 `MessageBoxW` allowing you to launch Vivaldi without mods.
- **Admin Elevation Built-in**: Seamlessly prompts Windows UAC when running `install-hook` or `uninstall-hook`.

---

## Directory Layout & Mod Storage

Mods and configuration are permanently stored in your Windows AppData directory, keeping them completely safe from browser updates:

```
%APPDATA%\VivaldiModManager\
├── mods_config.json          # Mod states and settings
└── user_mods\
    ├── css\                  # Permanent CSS mods
    └── js\                   # Permanent JS mods
```

---

## CLI Usage

### Basic Commands

```bash
# Setup: Detects Vivaldi, creates directories, and auto-imports existing mods
interceptor setup

# Check status (Vivaldi path, version, window.html patch state, IFEO hook state)
interceptor status
interceptor status --json

# List all registered mods and their enabled/disabled states
interceptor list-mods
interceptor list-mods --json

# Toggle a mod on or off
interceptor toggle BetterAnimation.css

# Import a new CSS or JS file
interceptor import C:\path\to\my_mod.js

# Manually trigger bundle recompilation and patching
interceptor patch

# Restore pristine original window.html
interceptor unpatch
```

### OS Integration (IFEO)

```bash
# Register Image File Execution Options (IFEO) hook in Windows Registry
# (Windows UAC prompt will appear automatically if not already elevated)
interceptor install-hook

# Remove IFEO hook from Windows Registry
interceptor uninstall-hook
```

Once installed, clicking any Vivaldi shortcut, opening links from external apps, or Vivaldi auto-restarting after an update will automatically route through `interceptor.exe`, verify/apply mods, and launch Vivaldi in milliseconds.

---

## Architecture Diagram

```
User Click / Link / Auto-Update
             │
             ▼
[Windows IFEO: vivaldi.exe]
             │
             ▼
      [interceptor.exe]
             │
      ┌──────┴────────────────────────────────────────┐
      ▼                                               ▼
[window.html has hook?]                        [window.html clean]
      │                                               │
      ├─ Yes ──► (Instant Pass-through)               ├─ No ──► Read %APPDATA% Mods
      │                                               │         Compile inject_bundle.{css,js}
      │                                               │         Inject script tag before </body>
      │                                               │         Backup window.html.orig
      ▼                                               ▼
[Win32 CreateProcess (DEBUG_ONLY_THIS_PROCESS)] ◄─────┘
      │
      ├─ DebugSetProcessKillOnExit(FALSE)
      ├─ DebugActiveProcessStop(processId)
      ▼
[vivaldi.exe runs completely detached]
[interceptor.exe exits immediately (~2-5ms total)]
```

---

## Building from Source

Prerequisites: Rust (stable toolchain)

```bash
cargo build --release
```

The optimized, standalone executable will be generated at `target\release\interceptor.exe` (approx. 1.6 MB with zero runtime dependencies).
