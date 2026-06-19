#!/usr/bin/env python3
"""Install unxml editor support (syntax highlighting for the .unxml format).

Installs every kind of support that applies on this machine:

  * bat        -- copies the Sublime grammar and rebuilds bat's cache.
  * VS Code    -- copies the language extension into every VS Code / VS Code
                  Insiders / Cursor extensions directory that exists, including
                  the Windows-side directory when run from WSL (grammar
                  extensions are "UI" extensions and must live on the local
                  side to highlight Remote-WSL files).

Works on WSL, native Windows, Linux and macOS. Re-running is safe (idempotent).

    python3 editor/install-editor-support.py

IMPORTANT for VS Code: fully QUIT VS Code (all windows) before running, then
start it again. A mere "Reload Window" may not pick up a newly copied
extension because VS Code caches its extension list.
"""

import glob
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SUBLIME = HERE / "unxml.sublime-syntax"
VSCODE_SRC = HERE / "vscode"
# VS Code's on-disk convention is <publisher>.<name>-<version>.
EXT_DIRNAME = "local.unxml-language-1.0.0"


def ok(msg):
    print(f"  [ok] {msg}")


def skip(msg):
    print(f"  [--] {msg}")


def warn(msg):
    print(f"  [!!] {msg}")


def is_wsl():
    if os.environ.get("WSL_DISTRO_NAME"):
        return True
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


def windows_user_homes():
    """Windows user-profile dirs reachable from WSL, as /mnt/... paths."""
    homes = []
    try:
        out = subprocess.run(
            ["cmd.exe", "/c", "echo %USERPROFILE%"],
            capture_output=True, text=True, cwd="/mnt/c",
        ).stdout.strip()
        if out and "%" not in out:
            wp = subprocess.run(
                ["wslpath", "-u", out], capture_output=True, text=True
            ).stdout.strip()
            if wp:
                homes.append(Path(wp))
    except (OSError, subprocess.SubprocessError):
        pass
    if not homes:  # fall back to scanning mounted drives
        for base in glob.glob("/mnt/*/Users/*"):
            p = Path(base)
            if (p / ".vscode").exists() or (p / ".cursor").exists():
                homes.append(p)
    return homes


# Editor variants keyed by the home-relative base dir they live under.
_EDITOR_BASES = [".vscode", ".vscode-insiders", ".vscode-server",
                 ".vscode-server-insiders", ".cursor", ".cursor-server"]


def vscode_ext_dirs():
    dirs = []
    home = Path.home()
    for base in _EDITOR_BASES:
        dirs.append(home / base / "extensions")
    if is_wsl():
        # Local (Windows) side -- where UI/grammar extensions must live to
        # affect Remote-WSL editing.
        for win_home in windows_user_homes():
            for base in (".vscode", ".vscode-insiders", ".cursor"):
                dirs.append(win_home / base / "extensions")
    # De-dup while preserving order.
    seen, unique = set(), []
    for d in dirs:
        if d not in seen:
            seen.add(d)
            unique.append(d)
    return unique


def install_vscode():
    print("VS Code:")
    installed = []
    for ext_dir in vscode_ext_dirs():
        # Only install where that editor variant actually exists.
        if not ext_dir.parent.exists():
            continue
        ext_dir.mkdir(parents=True, exist_ok=True)
        dest = ext_dir / EXT_DIRNAME
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(VSCODE_SRC, dest, ignore=shutil.ignore_patterns(".git"))
        # Invalidate the cached extension manifest so VS Code rescans the
        # folder on next start and discovers the new extension.
        for cache in ("extensions.json", ".init-default-profile-extensions"):
            p = ext_dir / cache
            try:
                p.unlink()
            except OSError:
                pass
        installed.append(dest)

    if not installed:
        skip("no VS Code / Cursor extensions directory found -- skipping")
        return
    for dest in installed:
        ok(f"installed to {dest}")
    print()
    print("  -> Fully QUIT VS Code (all windows), then start it again.")
    print("     Files ending in .unxml will then be highlighted.")


def main():
    if not SUBLIME.exists() or not VSCODE_SRC.exists():
        sys.exit(f"error: run from a checkout; missing files under {HERE}")
    print(f"Installing unxml editor support  (platform: {platform.system()}"
          f"{', WSL' if is_wsl() else ''})\n")
    install_bat()
    print()
    install_vscode()


if __name__ == "__main__":
    main()
