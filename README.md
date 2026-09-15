# Vivaldi Just-In-Time (JIT) Mod Interceptor

A blazingly fast Windows Just-In-Time (JIT) launch interceptor written in Rust that guarantees Vivaldi browser always launches with your custom CSS and JS mods applied—even immediately after automated or internal browser updates.

## Key Features

- **Instant JIT Patching (<5ms Overhead)**: Runs at execution time immediately before browser launch. Checks `window.html` and patches if missing.
- **Windows IFEO Infinite-Loop Prevention (Hardlink Bypass)**: Creates an instantaneous NTFS hardlink (`vivaldi_real.exe` -> `vivaldi.exe`) to bypass Windows Image File Execution Options without debugger overhead, process pausing, or recursion loops.
- **Dynamic Drive & Path Discovery**: Scans all active logical drives (A–Z) and registry paths. Interactively prompts to confirm detected installations or enter a custom path.
- **Chromium CSP-Compliant Bundling**: Concatenates enabled CSS and JS mods into `inject_bundle.css` and `inject_bundle.js` directly within Vivaldi's `resources\vivaldi\` package directory, avoiding Chromium extension Content Security Policy (CSP) errors.
- **Mod Isolation & Error Trapping**: Each JavaScript mod is executed in an isolated IIFE with dedicated `try ... catch` blocks so a syntax error or exception in one mod cannot break other mods or the Vivaldi UI.
- **UI Readiness Polling**: Automatically defers DOM-dependent scripts until Vivaldi's root `#browser` container is mounted (configurable selector and delay).
- **Pure Injection Policy**: Never touches or cleans up user-authored scripts or existing HTML tags. Only manages its own injection tag.
- **Clean Unpatch & Teardown**: `interceptor unpatch` restores pristine `window.html` and deletes `vivaldi_real.exe` along with compiled bundles.
- **Native Failsafe (Zero Lockout Risk)**: If patching ever fails (e.g. disk or permission error), displays a native Win32 `MessageBoxW` allowing you to launch Vivaldi without mods.
- **Admin Elevation Built-in**: Seamlessly prompts Windows UAC when running `install-hook` or `uninstall-hook`, forwarding parameters transparently.

---

## Directory Layout & Mod Storage

Mods and configuration can be stored in standard Windows AppData or in portable mode:

```
%APPDATA%\VivaldiModManager\ (or ./portable next to interceptor.exe)
├── mods_config.json          # Mod states, execution order, and settings
└── user_mods\
    ├── css\                  # Permanent CSS mods
    └── js\                   # Permanent JS mods
```

---

## CLI Usage

### Basic Commands

```bash
# Setup: Interactively confirms Vivaldi path, initializes storage, and imports mods
interceptor setup

# Setup in portable mode
interceptor setup --portable

# Check status (Vivaldi path, version, window.html patch state, IFEO hook state)
interceptor status
interceptor status --json

# List all registered mods, priority order, and their enabled/disabled states
interceptor list-mods
interceptor list-mods --json

# Toggle a mod on or off
interceptor toggle BetterAnimation.css

# Import a new CSS or JS file
interceptor import C:\path\to\my_mod.js

# Manually trigger bundle recompilation and patching
interceptor patch

# Restore pristine original window.html and remove hardlinks/bundles
interceptor unpatch
```

### OS Integration (IFEO)

```bash
# Register Image File Execution Options (IFEO) hook in Windows Registry
# (Prompts for target confirmation, then elevates via UAC)
interceptor install-hook

# Remove IFEO hook from Windows Registry and clean up hardlinks
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
      ├─ Yes ──► (Instant Pass-through)               ├─ No ──► Read Mods
      │                                               │         Compile inject_bundle.{css,js}
      │                                               │         Inject script tag before </body>
      │                                               │         Backup window.html.orig
      ▼                                               ▼
[Ensure NTFS Hardlink: vivaldi_real.exe] ◄────────────┘
      │
      ▼
[Win32 CreateProcessW("vivaldi_real.exe", <args>)]
      │ (Bypasses IFEO without recursion or debugging flags)
      ▼
[Vivaldi Browser Runs at Native Speed]
[interceptor.exe exits immediately (~2-5ms total)]
```

---

## Building from Source

Prerequisites: Rust (stable toolchain)

```bash
cargo build --release
```

The optimized, standalone executable will be generated at `target\release\interceptor.exe` (approx. 1.6 MB with zero runtime dependencies).
