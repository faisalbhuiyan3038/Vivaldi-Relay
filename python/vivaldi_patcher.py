#!/usr/bin/env python3
"""
Vivaldi one-shot mod patcher.

Patches a Vivaldi version folder once with the CSS/JS mods found in ./css and
./js (or a custom mods dir). No hooks, no background process: run it after an
update to re-apply, run `unpatch` to revert.

    python vivaldi_patcher.py            interactive menu
    python vivaldi_patcher.py patch      apply mods
    python vivaldi_patcher.py unpatch    revert to stock
    python vivaldi_patcher.py order      reorder / enable / disable mods
    python vivaldi_patcher.py status     show what is patched
"""

# ============================ USER CONFIG ===================================
# Full path to vivaldi.exe on this machine.
VIVALDI_EXE = r"C:\Users\YOUR_NAME\AppData\Local\Vivaldi\Application\vivaldi.exe"

# Folder containing the `css` and `js` mod folders. None = next to this script.
MODS_DIR = None

# Built-in mod: let tabs stretch to fill the tab bar (patches Vivaldi's bundle.js).
STRETCH_TABS = False

# JS mods run once this selector exists in the Vivaldi UI, after this delay.
UI_READY_SELECTOR = "#browser"
INIT_DELAY_MS = 60
# ============================================================================

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

VERSION_RE = re.compile(r"^\d+(\.\d+){3}$")

BUNDLE_CSS = "vivaldi-mods.css"
BUNDLE_JS = "vivaldi-mods.js"
BACKUP_SUFFIX = ".vmp-orig"
ORDER_FILE = "order.txt"

MARK_BEGIN = "<!-- vivaldi-patcher:begin -->"
MARK_END = "<!-- vivaldi-patcher:end -->"
HOOK_BLOCK = f'{MARK_BEGIN}<script src="{BUNDLE_JS}"></script>{MARK_END}'
HOOK_INSERT = f"  {HOOK_BLOCK}\n"
HOOK_RE = re.compile(re.escape(MARK_BEGIN) + r".*?" + re.escape(MARK_END) + r"[ \t]*\r?\n?", re.S)

# Legacy Rust interceptor markers (reported only, never touched).
LEGACY_HOOK_ID = "inject_bundle.js"
IFEO_KEY = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\vivaldi.exe"

# Same as src/bundle_patcher.rs: (?<![-+!<>=*/%&^|?])=180(?=[;,].{0,1000}isDraggingPinnedTab)
TAB_WIDTH_OLD = b"=180"
TAB_WIDTH_NEW = b"=5200"
TAB_WIDTH_RE = re.compile(rb"(?<![-+!<>=*/%&^|?])=180(?=[;,].{0,1000}isDraggingPinnedTab)", re.S)
TAB_WIDTH_PATCHED_RE = re.compile(rb"(?<![-+!<>=*/%&^|?])=5200(?=[;,].{0,1000}isDraggingPinnedTab)", re.S)


class PatchError(Exception):
    pass


# ---------------------------------------------------------------- utilities

def info(msg=""):
    print(msg)


def warn(msg):
    print(f"  ! {msg}")


def read_text(path):
    with open(path, "r", encoding="utf-8", newline="") as f:
        return f.read()


def write_text_atomic(path, content):
    tmp = path.with_name(path.name + ".tmp")
    with open(tmp, "w", encoding="utf-8", newline="") as f:
        f.write(content)
    os.replace(tmp, path)


def write_bytes_atomic(path, data):
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_bytes(data)
    os.replace(tmp, path)


def ask(prompt):
    try:
        return input(prompt).strip()
    except EOFError:
        return ""


def version_key(name):
    return tuple(int(p) for p in name.split("."))


# ---------------------------------------------------------------- discovery

class Target:
    def __init__(self, version_dir):
        self.version = version_dir.name
        self.resources = version_dir / "resources" / "vivaldi"
        self.window_html = self.resources / "window.html"
        self.bundle_js = self.resources / "bundle.js"

    def backup_of(self, path):
        return path.with_name(path.name + BACKUP_SUFFIX)

    def is_patched(self):
        try:
            return MARK_BEGIN in read_text(self.window_html)
        except OSError:
            return False

    def has_legacy_hook(self):
        try:
            return LEGACY_HOOK_ID in read_text(self.window_html)
        except OSError:
            return False

    def stretch_tabs_state(self):
        if not self.bundle_js.is_file():
            return "bundle.js missing"
        data = self.bundle_js.read_bytes()
        if TAB_WIDTH_PATCHED_RE.search(data):
            return "on"
        if TAB_WIDTH_RE.search(data):
            return "off"
        return "pattern not found"


def find_targets(app_dir):
    if not app_dir.is_dir():
        raise PatchError(f"Vivaldi Application folder not found: {app_dir}\n"
                         f"  Set VIVALDI_EXE at the top of this script or pass --vivaldi.")
    targets = [Target(d) for d in app_dir.iterdir()
               if d.is_dir() and VERSION_RE.match(d.name)]
    targets = [t for t in targets if t.window_html.is_file()]
    targets.sort(key=lambda t: version_key(t.version), reverse=True)
    if not targets:
        raise PatchError(f"No Vivaldi version folders (e.g. 7.5.2.3) with "
                         f"resources\\vivaldi\\window.html found in {app_dir}")
    return targets


def choose_target(targets, wanted_version, action):
    if wanted_version:
        for t in targets:
            if t.version == wanted_version:
                return t
        raise PatchError(f"Version {wanted_version} not found. Available: "
                         + ", ".join(t.version for t in targets))
    if len(targets) == 1:
        return targets[0]

    info(f"\nMultiple Vivaldi versions found (an update is probably pending). Which one to {action}?")
    for i, t in enumerate(targets, 1):
        tags = ["newest"] if i == 1 else []
        tags.append("patched" if t.is_patched() else "not patched")
        info(f"  {i}. {t.version}  ({', '.join(tags)})")
    while True:
        choice = ask(f"Select [1-{len(targets)}, Enter = 1, q = cancel]: ")
        if choice == "":
            return targets[0]
        if choice.lower() == "q":
            raise PatchError("Cancelled.")
        if choice.isdigit() and 1 <= int(choice) <= len(targets):
            return targets[int(choice) - 1]
        info("  Invalid choice.")


# ---------------------------------------------------------------- mod ordering

class ModList:
    """One mods folder (css or js) and its order.txt."""

    def __init__(self, folder, ext):
        self.folder = folder
        self.ext = ext
        self.order_file = folder / ORDER_FILE
        self.entries = []  # list of [name, enabled]

    def load(self):
        """Read order.txt and reconcile it with the files on disk."""
        self.folder.mkdir(parents=True, exist_ok=True)
        on_disk = {p.name for p in self.folder.iterdir()
                   if p.is_file() and p.suffix.lower() == self.ext}

        listed = []
        seen = set()
        if self.order_file.is_file():
            for raw in read_text(self.order_file).splitlines():
                line = raw.strip()
                if not line:
                    continue
                enabled = not line.startswith("#")
                name = line.lstrip("#").strip()
                if name in on_disk and name not in seen:
                    listed.append([name, enabled])
                    seen.add(name)

        for name in sorted(on_disk - seen, key=str.lower):
            listed.append([name, True])

        self.entries = listed
        return self

    def save(self):
        lines = [name if enabled else f"# {name}" for name, enabled in self.entries]
        write_text_atomic(self.order_file, "\n".join(lines) + ("\n" if lines else ""))

    def enabled(self):
        return [self.folder / name for name, enabled in self.entries if enabled]


def load_mods(mods_dir):
    css = ModList(mods_dir / "css", ".css").load()
    js = ModList(mods_dir / "js", ".js").load()
    css.save()
    js.save()
    return css, js


# ---------------------------------------------------------------- bundling

def build_css(css_files):
    parts = ["/* [Vivaldi one-shot patcher - compiled stylesheet] */\n"]
    for path in css_files:
        parts.append(f"\n/* === mod: {path.name} === */\n")
        parts.append(read_text(path))
        parts.append("\n")
    return "".join(parts)


def js_string(s):
    return "'" + s.replace("\\", "\\\\").replace("'", "\\'") + "'"


def build_js(js_files, css_count):
    out = [f"""// [Vivaldi one-shot patcher - JavaScript bootstrap]
(function() {{
    'use strict';

    // 1. Inject compiled stylesheet into <head>
    if (!document.getElementById('vivaldi-patcher-css')) {{
        const link = document.createElement('link');
        link.id = 'vivaldi-patcher-css';
        link.rel = 'stylesheet';
        link.href = '{BUNDLE_CSS}';
        (document.head || document.documentElement).appendChild(link);
    }}

    // 2. Run each mod in its own scope so one failure can't break the rest
    function executeMods() {{
"""]
    for path in js_files:
        name = js_string(path.name)
        out.append(f"""        // --- mod: {path.name} ---
        try {{
            (function() {{
{read_text(path)}
            }})();
        }} catch (err) {{
            console.error('[vivaldi-patcher] Error in mod ' + {name} + ':', err);
        }}

""")
    out.append(f"""        console.info('[vivaldi-patcher] Loaded {css_count} CSS and {len(js_files)} JS mods.');
    }}

    // 3. Wait for the Vivaldi UI before running DOM mods
    function waitForUI() {{
        if (document.querySelector({js_string(UI_READY_SELECTOR)})) {{
            setTimeout(executeMods, {int(INIT_DELAY_MS)});
        }} else {{
            setTimeout(waitForUI, 200);
        }}
    }}

    if (document.readyState === 'loading') {{
        document.addEventListener('DOMContentLoaded', waitForUI);
    }} else {{
        waitForUI();
    }}
}})();
""")
    return "".join(out)


# ---------------------------------------------------------------- window.html

def inject_hook(html):
    if MARK_BEGIN in html:
        return html
    pos = html.lower().rfind("</body>")
    if pos == -1:
        return html + ("" if html.endswith("\n") else "\n") + HOOK_INSERT
    return html[:pos] + HOOK_INSERT + html[pos:]


def remove_hook(html):
    if HOOK_INSERT in html:
        return html.replace(HOOK_INSERT, "")
    return HOOK_RE.sub("", html)


# ---------------------------------------------------------------- stretch tabs

def apply_stretch_tabs(target, enable):
    """Bring bundle.js in line with the desired stretch-tabs state."""
    bundle = target.bundle_js
    backup = target.backup_of(bundle)
    if not bundle.is_file():
        if enable:
            warn(f"Stretch tabs: bundle.js not found in {target.resources}, skipped.")
        return

    if not enable:
        if backup.is_file():
            os.replace(backup, bundle)
            info("  Stretch tabs: restored original bundle.js")
        return

    data = bundle.read_bytes()
    if TAB_WIDTH_PATCHED_RE.search(data):
        info("  Stretch tabs: already applied")
        return
    match = TAB_WIDTH_RE.search(data)
    if not match:
        warn("Stretch tabs: max tab width pattern not found in bundle.js "
             "(Vivaldi may have changed its code). Skipped.")
        return
    backup.write_bytes(data)
    write_bytes_atomic(bundle, data[:match.start()] + TAB_WIDTH_NEW + data[match.end():])
    info("  Stretch tabs: applied")


# ---------------------------------------------------------------- actions

def do_patch(ctx):
    target = choose_target(find_targets(ctx.app_dir), ctx.version, "patch")
    css, js = load_mods(ctx.mods_dir)
    css_files, js_files = css.enabled(), js.enabled()

    info(f"\nPatching Vivaldi {target.version}")
    info(f"  Mods folder: {ctx.mods_dir}")
    print_mod_lists(css, js, indent="  ")
    if not css_files and not js_files and not ctx.stretch_tabs:
        warn("No enabled mods. Bundles will be empty.")

    write_text_atomic(target.resources / BUNDLE_CSS, build_css(css_files))
    write_text_atomic(target.resources / BUNDLE_JS, build_js(js_files, len(css_files)))

    html_path = target.window_html
    html = read_text(html_path)
    backup = target.backup_of(html_path)
    if MARK_BEGIN not in html:
        # Unpatched file is the pristine baseline, so (re)take the backup now.
        with open(backup, "w", encoding="utf-8", newline="") as f:
            f.write(html)
        write_text_atomic(html_path, inject_hook(html))
        info("  window.html: hook added")
    else:
        info("  window.html: hook already present, bundles rebuilt")

    apply_stretch_tabs(target, ctx.stretch_tabs)

    if target.has_legacy_hook():
        warn("window.html also contains the old Rust interceptor hook (inject_bundle.js). "
             "Mods may load twice. Run `interceptor unpatch` to remove it.")
    info(f"\nDone. {len(css_files)} CSS + {len(js_files)} JS mods applied to {target.version}.")
    if vivaldi_running():
        info("Vivaldi is running: restart it to see the changes.")


def do_unpatch(ctx):
    target = choose_target(find_targets(ctx.app_dir), ctx.version, "unpatch")
    info(f"\nUnpatching Vivaldi {target.version}")

    html_path = target.window_html
    backup = target.backup_of(html_path)
    html = read_text(html_path)
    if MARK_BEGIN in html:
        cleaned = remove_hook(html)
        if MARK_BEGIN in cleaned:
            if not backup.is_file():
                raise PatchError("Could not remove the hook from window.html and no backup exists.")
            os.replace(backup, html_path)
            info("  window.html: restored from backup")
        else:
            write_text_atomic(html_path, cleaned)
            info("  window.html: hook removed")
    else:
        info("  window.html: not patched")
    if backup.is_file():
        backup.unlink()

    for name in (BUNDLE_CSS, BUNDLE_JS):
        p = target.resources / name
        if p.is_file():
            p.unlink()
            info(f"  removed {name}")

    apply_stretch_tabs(target, enable=False)
    info(f"\nDone. Vivaldi {target.version} is back to stock.")
    if vivaldi_running():
        info("Vivaldi is running: restart it to see the changes.")


def print_mod_lists(css, js, indent=""):
    for label, mods in (("CSS", css), ("JS", js)):
        if not mods.entries:
            info(f"{indent}{label}: (none)")
            continue
        info(f"{indent}{label}:")
        for i, (name, enabled) in enumerate(mods.entries, 1):
            info(f"{indent}  {i:2}. {'' if enabled else '[off] '}{name}")


def do_status(ctx):
    info(f"vivaldi.exe : {ctx.vivaldi_exe}")
    info(f"Mods folder : {ctx.mods_dir}")
    info(f"Stretch tabs: {'enabled' if ctx.stretch_tabs else 'disabled'} (config)")
    info("")
    for t in find_targets(ctx.app_dir):
        line = f"  {t.version}: {'PATCHED' if t.is_patched() else 'not patched'}"
        line += f", stretch tabs {t.stretch_tabs_state()}"
        if t.has_legacy_hook():
            line += ", OLD RUST HOOK PRESENT"
        info(line)
    info("")
    css, js = load_mods(ctx.mods_dir)
    print_mod_lists(css, js)
    check_legacy(ctx)


def do_order(ctx):
    css, js = load_mods(ctx.mods_dir)
    changed = False
    while True:
        info("\nReorder which list?  1. CSS   2. JS   q. done")
        choice = ask("> ").lower()
        if choice in ("q", ""):
            break
        mods = {"1": css, "2": js}.get(choice)
        if mods is None:
            continue
        changed |= edit_order(mods, "CSS" if mods is css else "JS")

    if changed and ask("\nRe-patch Vivaldi now with the new order? [y/N] ").lower() == "y":
        do_patch(ctx)


def edit_order(mods, label):
    """Interactive editor for one ModList. Returns True if saved."""
    original = [e[:] for e in mods.entries]
    while True:
        info(f"\n{label} mods (top loads first):")
        if not mods.entries:
            info(f"  (no {mods.ext} files in {mods.folder})")
            return False
        for i, (name, enabled) in enumerate(mods.entries, 1):
            info(f"  {i:2}. {'' if enabled else '[off] '}{name}")
        info("Commands: m <n> <pos> = move,  t <n> = toggle on/off,  s = save,  q = back without saving")
        parts = ask("> ").split()
        if not parts:
            continue
        cmd, args = parts[0].lower(), parts[1:]
        n = len(mods.entries)
        try:
            nums = [int(a) for a in args]
        except ValueError:
            info("  Numbers please.")
            continue
        if cmd == "m" and len(nums) == 2 and all(1 <= x <= n for x in nums):
            entry = mods.entries.pop(nums[0] - 1)
            mods.entries.insert(nums[1] - 1, entry)
        elif cmd == "t" and len(nums) == 1 and 1 <= nums[0] <= n:
            mods.entries[nums[0] - 1][1] = not mods.entries[nums[0] - 1][1]
        elif cmd == "s":
            mods.save()
            info(f"  Saved {mods.order_file}")
            return True
        elif cmd == "q":
            mods.entries = original
            return False
        else:
            info("  Invalid command.")


# ---------------------------------------------------------------- environment checks

def vivaldi_running():
    if os.name != "nt":
        return False
    try:
        out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq vivaldi.exe", "/NH"],
                             capture_output=True, text=True, timeout=10).stdout
        return "vivaldi.exe" in out.lower()
    except (OSError, subprocess.SubprocessError):
        return False


def check_legacy(ctx):
    if os.name == "nt":
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, IFEO_KEY) as key:
                debugger, _ = winreg.QueryValueEx(key, "Debugger")
            warn(f"The old IFEO hook is still installed (Debugger = {debugger}).")
            warn("It intercepts every Vivaldi launch and re-patches new installs. "
                 "Remove it with: interceptor uninstall-hook")
        except OSError:
            pass
    if (ctx.app_dir / "vivaldi_real.exe").exists():
        warn("vivaldi_real.exe (old interceptor hardlink) exists. "
             "`interceptor uninstall-hook` removes it.")


# ---------------------------------------------------------------- entry point

class Context:
    def __init__(self, args):
        self.vivaldi_exe = Path(args.vivaldi or VIVALDI_EXE)
        self.app_dir = self.vivaldi_exe.parent
        mods = args.mods or MODS_DIR
        self.mods_dir = Path(mods) if mods else Path(__file__).resolve().parent
        self.version = args.version
        self.stretch_tabs = STRETCH_TABS if args.stretch_tabs is None else args.stretch_tabs


def menu(ctx):
    actions = {"1": do_patch, "2": do_unpatch, "3": do_order, "4": do_status}
    while True:
        info("\n=== Vivaldi mod patcher ===")
        info("  1. Patch\n  2. Unpatch\n  3. Reorder / toggle mods\n  4. Status\n  q. Quit")
        choice = ask("> ").lower()
        if choice in ("q", ""):
            return
        action = actions.get(choice)
        if action:
            run(action, ctx)


def run(action, ctx):
    try:
        action(ctx)
        return 0
    except PatchError as e:
        print(f"\nError: {e}")
    except PermissionError as e:
        print(f"\nPermission denied: {e.filename or e}")
        print("Close Vivaldi, or run this script from an Administrator terminal "
              "if Vivaldi is installed under Program Files.")
    return 1


def main():
    parser = argparse.ArgumentParser(description="One-shot Vivaldi mod patcher.")
    parser.add_argument("command", nargs="?", choices=["patch", "unpatch", "order", "status"],
                        help="omit for the interactive menu")
    parser.add_argument("--vivaldi", help="path to vivaldi.exe (overrides VIVALDI_EXE)")
    parser.add_argument("--mods", help="folder containing css/ and js/ (overrides MODS_DIR)")
    parser.add_argument("--version", help="Vivaldi version folder to use, e.g. 7.5.2.3")
    stretch = parser.add_mutually_exclusive_group()
    stretch.add_argument("--stretch-tabs", dest="stretch_tabs", action="store_true", default=None)
    stretch.add_argument("--no-stretch-tabs", dest="stretch_tabs", action="store_false")
    args = parser.parse_args()

    ctx = Context(args)
    if args.command is None:
        check_legacy(ctx)
        menu(ctx)
        return 0
    if args.command in ("patch", "unpatch"):
        check_legacy(ctx)
    commands = {"patch": do_patch, "unpatch": do_unpatch, "order": do_order, "status": do_status}
    return run(commands[args.command], ctx)


if __name__ == "__main__":
    sys.exit(main())
