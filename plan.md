# Implementation Plan: Vivaldi JIT Mod Interceptor
**Target Platform:** Windows (First Pass)
**Core Tech Stack:** Rust (Interceptor CLI), JavaScript (Bootstrap Payload)

## Phase 1: Scaffold Project & Configuration Layer
**Goal:** Set up the Rust workspace, CLI routing, and the local state management.
1. Initialize a new Rust binary project (`cargo new vivaldi_mod_interceptor`).
2. Add dependencies: `clap` (CLI args), `serde` / `serde_json` (config parsing), `directories` (OS paths), and `anyhow` (error handling).
3. Define the config schema (`ModConfig`) representing enabled/disabled mods.
4. Implement a configuration manager that reads/writes to `%APPDATA%\VivaldiModManager\mods_config.json`.
5. Create basic CLI commands: `interceptor setup`, `interceptor launch`, and `interceptor toggle <mod_name>`.
*Checkpoint:* The agent should be able to run CLI commands to toggle boolean states in the JSON config file.

## Phase 2: Vivaldi Discovery & Patching Engine
**Goal:** Safely locate Vivaldi, read `window.html`, and inject the bootstrap hook idempontently.
1. Implement Vivaldi discovery logic. Check default installation paths (`%LOCALAPPDATA%\Vivaldi\Application` and `%PROGRAMFILES%\Vivaldi\Application`) to find the latest versioned folder containing `resources\vivaldi\window.html`.
2. Write the patcher function:
   - Read `window.html` into memory.
   - Check if the file already contains the `bootstrap.js` script tag (idempotency check).
   - If missing, inject `<script src="file:///[APPDATA_PATH]/bootstrap.js"></script>` immediately before the `</body>` tag.
   - Write the file back securely.
3. Write unit tests for the patcher using mock HTML files to ensure it doesn't corrupt valid HTML or double-inject.
*Checkpoint:* Running `interceptor patch-test` should successfully modify a dummy `window.html` file.

## Phase 3: The JavaScript Bootstrap Payload
**Goal:** Create the stable script that lives outside the Vivaldi directory and loads the mods.
1. Create `bootstrap.js` inside `%APPDATA%\VivaldiModManager\`.
2. Write logic in `bootstrap.js` to synchronously (or via `fetch`) read `mods_config.json`.
3. Implement DOM injection in `bootstrap.js`:
   - Iterate through enabled mods in the JSON.
   - Create and append `<link rel="stylesheet">` tags for CSS.
   - Create and append `<script>` tags for JS.
4. Ensure the Rust setup command automatically deploys this `bootstrap.js` file to the AppData directory on first run.
*Checkpoint:* Manually pasting the script tag into Vivaldi's `window.html` should successfully execute `bootstrap.js` and load a test CSS mod (e.g., changing the background color red).

## Phase 4: Launch Interception & Execution (The JIT Core)
**Goal:** Execute the patch-and-launch sequence with full argument pass-through.
1. In the Rust `launch` command, sequence the flow:
   - Call discovery & patching engine (Phase 2).
   - Capture all trailing CLI arguments passed to the interceptor (e.g., URLs, `--incognito`).
   - Use `std::process::Command` to spawn the actual `vivaldi.exe`, passing along all captured arguments.
   - Exit the Rust process immediately after the Vivaldi process detaches.
*Checkpoint:* Running `interceptor launch https://example.com` should patch the browser (if needed) and open Vivaldi to that URL.

## Phase 5: OS Integration (IFEO) & Failsafes
**Goal:** Route Vivaldi shortcuts through the interceptor and handle failures gracefully.
1. Add the `winreg` crate to manage Windows Registry keys.
2. Implement the `interceptor install-hook` command (requires Admin):
   - Set `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\vivaldi.exe`
   - Add String Value: `Debugger` = `"[Path_To_Interceptor.exe] launch"`
3. Implement fallback/failsafe logic:
   - If the patcher fails (e.g., locked file, path not found), use the `msgbox` crate to show a native Windows error dialog ("Mod injection failed. Continue launching Vivaldi anyway?").
   - If the user clicks Yes, proceed to launch `vivaldi.exe` without patching.
*Checkpoint:* Clicking a standard Vivaldi desktop shortcut should trigger the interceptor, run the JIT check, and open Vivaldi seamlessly.

## Phase 6: Management API (Prep for GUI)
**Goal:** Finalize the tool so a GUI (like Tauri) can easily interface with it.
1. Ensure the Rust binary can output JSON responses for CLI commands (e.g., `interceptor list-mods --json`).
2. Add a command to import a new mod (copying `.css`/`.js` files into `%APPDATA%\VivaldiModManager\user_mods\` and registering it in the JSON config).
*Checkpoint:* The CLI is now fully feature-complete and ready for a frontend.