#!/usr/bin/env python3
"""Install unxml editor support (syntax highlighting for the .unxml format).

Installs bat support: copies the Sublime grammar and rebuilds bat's cache.
Superseded by `unxml --install-bat`, which does the same thing without a
checkout on disk (the grammar is embedded in the binary) — prefer that from
a plain `cargo install`. This script remains for a checkout that has no
`unxml` binary built yet.

Works on WSL, native Windows, Linux and macOS. Re-running is safe (idempotent).

    python3 editor/install-editor-support.py

VS Code / Cursor support is no longer shipped here. Install the **JadeView**
extension instead, which bundles the .unxml grammar (and adds HTML->Pug and
unxml rendering commands):

    https://github.com/vivainio/jadeview
"""

import platform
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SUBLIME = HERE / "unxml.sublime-syntax"
JADEVIEW_URL = "https://github.com/vivainio/jadeview"


def ok(msg):
    print(f"  [ok] {msg}")


def skip(msg):
    print(f"  [--] {msg}")


def warn(msg):
    print(f"  [!!] {msg}")


def is_wsl():
    try:
        return "microsoft" in Path("/proc/version").read_text().lower()
    except OSError:
        return False


# --------------------------------------------------------------------------- bat


def install_bat():
    print("bat:")
    bat = shutil.which("bat") or shutil.which("batcat")
    if not bat:
        skip("bat not found on PATH -- skipping")
        return
    try:
        cfg = subprocess.run(
            [bat, "--config-dir"], capture_output=True, text=True, check=True
        ).stdout.strip()
    except subprocess.CalledProcessError as e:
        warn(f"could not query 'bat --config-dir': {e}")
        return
    dest = Path(cfg) / "syntaxes"
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copy(SUBLIME, dest / SUBLIME.name)
    ok(f"copied grammar to {dest}")
    try:
        subprocess.run([bat, "cache", "--build"], check=True,
                       capture_output=True, text=True)
        ok("rebuilt bat cache  (use: bat file.unxml)")
    except subprocess.CalledProcessError as e:
        warn(f"'bat cache --build' failed: {e.stderr or e}")


# ----------------------------------------------------------------------- VS Code


def note_vscode():
    print("VS Code / Cursor:")
    skip("install the JadeView extension -- it bundles the .unxml grammar")
    print(f"     {JADEVIEW_URL}")


def main():
    if not SUBLIME.exists():
        sys.exit(f"error: run from a checkout; missing files under {HERE}")
    print(f"Installing unxml editor support  (platform: {platform.system()}"
          f"{', WSL' if is_wsl() else ''})\n")
    install_bat()
    print()
    note_vscode()


if __name__ == "__main__":
    main()
