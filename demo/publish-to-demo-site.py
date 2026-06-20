#!/usr/bin/env python3
"""Render the checked-in demo/*.unxml files to syntax-highlighted HTML pages
for the Zensical demo site (https://github.com/vivainio/unxml-demos).

Pipeline: each `.unxml` file is highlighted by `bat` using the installed
`unxml` grammar (so the colours match the terminal exactly), then the ANSI
output is converted to class-based HTML by `ansi2html`. All files share one
generated stylesheet so the pages stay small.

Requirements:
  - `bat` on PATH with the unxml grammar installed
    (run `python3 editor/install-editor-support.py` in unxml-rs first)
  - `pip install ansi2html`

Usage:
  python3 demo/publish-to-demo-site.py [PATH_TO_unxml-demos]

PATH defaults to ../unxml-demos relative to this repo.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

try:
    from ansi2html import Ansi2HTMLConverter
except ImportError:
    sys.exit("ansi2html not installed. Run: pip install ansi2html")

# Repo layout: this script lives in <repo>/demo/.
DEMO_DIR = Path(__file__).resolve().parent
REPO_ROOT = DEMO_DIR.parent

# Per-file metadata: relative path under demo/ -> (page title, source URL).
# Anything not listed still renders, with a title derived from the filename.
UBL = "https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd"
SOURCES: dict[str, tuple[str, str]] = {
    "finvoice-3.0.xsd.unxml": (
        "Finvoice 3.0",
        "https://file.finanssiala.fi/finvoice/Finvoice3.0.xsd",
    ),
    "ubl/cct.xsd.unxml": ("UBL — Core Component Types", f"{UBL}/common/CCTS_CCT_SchemaModule-2.1.xsd"),
    "ubl/udt.xsd.unxml": ("UBL — Unqualified Data Types", f"{UBL}/common/UBL-UnqualifiedDataTypes-2.1.xsd"),
    "ubl/qdt.xsd.unxml": ("UBL — Qualified Data Types", f"{UBL}/common/UBL-QualifiedDataTypes-2.1.xsd"),
    "ubl/cbc.xsd.unxml": ("UBL — Common Basic Components", f"{UBL}/common/UBL-CommonBasicComponents-2.1.xsd"),
    "ubl/cac.xsd.unxml": ("UBL — Common Aggregate Components", f"{UBL}/common/UBL-CommonAggregateComponents-2.1.xsd"),
    "ubl/cec.xsd.unxml": ("UBL — Common Extension Components", f"{UBL}/common/UBL-CommonExtensionComponents-2.1.xsd"),
    "ubl/invoice.xsd.unxml": ("UBL — Invoice", f"{UBL}/maindoc/UBL-Invoice-2.1.xsd"),
}

# Extra CSS appended to the generated stylesheet so the highlighted blocks
# read as proper code panels regardless of the surrounding page theme.
PANEL_CSS = """
/* unxml code panels (highlighting itself is generated above) */
pre.unxml-demo {
  background: #0d1117;
  color: #c9d1d9;
  padding: 1rem 1.25rem;
  border-radius: 8px;
  overflow-x: auto;
  font-size: 0.8rem;
  line-height: 1.45;
  tab-size: 2;
}
pre.unxml-demo .ansi2html-content { white-space: pre; }
"""


def find_bat() -> str:
    for name in ("bat", "batcat"):
        try:
            subprocess.run([name, "--version"], capture_output=True, check=True)
            return name
        except (FileNotFoundError, subprocess.CalledProcessError):
            continue
    sys.exit("bat not found on PATH.")


def highlight(bat: str, path: Path) -> str:
    """Return ANSI-highlighted text for one .unxml file."""
    result = subprocess.run(
        [bat, "--color=always", "--paging=never", "--style=plain",
         "--wrap=never", "-l", "unxml", str(path)],
        capture_output=True, text=True, check=True,
    )
    return result.stdout


def discover() -> list[Path]:
    """All checked-in .unxml files under demo/, sorted with known ones first."""
    files = sorted(DEMO_DIR.rglob("*.unxml"))
    order = list(SOURCES)
    return sorted(files, key=lambda p: (
        order.index(rel) if (rel := p.relative_to(DEMO_DIR).as_posix()) in order
        else len(order), p.as_posix(),
    ))


def page_markdown(rel: str, fragment: str, lines: int) -> str:
    title, source = SOURCES.get(rel, (Path(rel).stem, None))
    out = [f"# {title}\n"]
    src = f"[`{Path(rel).name}`]({source})" if source else f"`{Path(rel).name}`"
    out.append(f"Source: {src} — {lines} lines of `unxml --xsd` output.\n")
    # Raw HTML block; md_in_html is enabled in zensical.toml so it passes through.
    out.append('<pre class="unxml-demo" markdown="0"><span class="ansi2html-content">')
    out.append(fragment.rstrip("\n"))
    out.append("</span></pre>\n")
    return "\n".join(out)


def index_markdown(entries: list[tuple[str, str, int]]) -> str:
    out = [
        "# unxml demos\n",
        "Real-world XML Schemas rendered with [`unxml --xsd`]"
        "(https://github.com/vivainio/unxml-rs), syntax-highlighted with the "
        "same grammar `unxml` ships for `bat`.\n",
        "| Schema | Lines |",
        "| --- | --- |",
    ]
    for slug, title, lines in entries:
        out.append(f"| [{title}]({slug}.md) | {lines} |")
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "target", nargs="?", default=str(REPO_ROOT.parent / "unxml-demos"),
        help="path to the unxml-demos repo (default: ../unxml-demos)",
    )
    args = ap.parse_args()

    site = Path(args.target).resolve()
    docs = site / "docs"
    if not docs.is_dir():
        sys.exit(f"{docs} not found — is {site} the unxml-demos repo?")

    bat = find_bat()
    conv = Ansi2HTMLConverter(inline=False, dark_bg=True, scheme="xterm")

    demos_dir = docs / "demos"
    entries: list[tuple[str, str, int]] = []  # (slug, title, lines)

    for path in discover():
        rel = path.relative_to(DEMO_DIR).as_posix()
        ansi = highlight(bat, path)
        fragment = conv.convert(ansi, full=False)
        # Markdown (even inside raw HTML) scans bracket text like `[0..2]` or
        # `[A-Z0-9]` as link labels and warns about unresolved references.
        # Entity-encode the brackets: browsers still show them, Markdown can't.
        # ansi2html uses no brackets in markup, so this only touches displayed
        # text. (Per-token spans already protect */_/backtick from emphasis.)
        fragment = fragment.replace("[", "&#91;").replace("]", "&#93;")
        lines = path.read_text(encoding="utf-8").count("\n")

        # Mirror demo/'s layout: demo/ubl/foo.unxml -> docs/demos/ubl/foo.md
        slug = rel.removesuffix(".unxml").replace(".xsd", "").replace(".", "-")
        out_path = demos_dir / f"{slug}.md"
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(page_markdown(rel, fragment, lines), encoding="utf-8")

        title = SOURCES.get(rel, (Path(rel).stem, None))[0]
        entries.append((slug, title, lines))
        print(f"  rendered {rel} -> {out_path.relative_to(site)} ({lines} lines)")

    # Section index for the Demos nav group.
    (demos_dir / "index.md").write_text(index_markdown(entries), encoding="utf-8")

    # Shared stylesheet (every colour class used across all files, since we
    # reused one converter), plus the code-panel chrome. produce_headers()
    # wraps the rules in a <style> block; strip it for a standalone .css.
    headers = conv.produce_headers()
    css = headers.replace('<style type="text/css">', "").replace("</style>", "").strip()
    css += "\n" + PANEL_CSS
    css_path = docs / "stylesheets" / "unxml.css"
    css_path.parent.mkdir(parents=True, exist_ok=True)
    css_path.write_text(css, encoding="utf-8")
    print(f"  wrote stylesheet -> {css_path.relative_to(site)}")

    print(f"\nDone. {len(entries)} pages written to {demos_dir.relative_to(site)}/.")
    print("Ensure zensical.toml has: extra_css = [\"stylesheets/unxml.css\"]")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
