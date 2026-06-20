#!/usr/bin/env python3
"""Render the checked-in demo/*.unxml files to syntax-highlighted HTML pages
for the Zensical demo site (https://github.com/vivainio/unxml-demos).

Each `.unxml` file is highlighted by `bat` (using the installed `unxml`
grammar, so colours match the terminal exactly), then the ANSI output is
turned into class-based HTML by `ansi2html`. Every file is written as a
self-contained, full-page HTML document under docs/demos/ — Zensical copies
non-Markdown files through verbatim, so they render edge-to-edge with no
site chrome. A themed Markdown index page links to them.

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
import re
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

# Page chrome shared by every standalone demo: a dark, edge-to-edge,
# horizontally-scrolling code surface plus the floating "back" link. This is
# appended to ansi2html's generated colour classes in one shared stylesheet
# (demos/unxml.css) that every page links to.
CHROME_CSS = """
html, body { margin: 0; background: #0d1117; }
pre.unxml {
  margin: 0;
  padding: 1.25rem 1.5rem;
  color: #c9d1d9;
  font: 13px/1.5 ui-monospace, "SF Mono", SFMono-Regular, Menlo, Consolas, monospace;
  white-space: pre;
  tab-size: 2;
}
.unxml-back {
  position: fixed; top: 0; right: 0;
  padding: 0.4rem 0.8rem; margin: 0.5rem;
  background: #161b22; color: #8b949e; border-radius: 6px;
  font: 12px/1 ui-monospace, monospace; text-decoration: none;
}
.unxml-back:hover { color: #c9d1d9; }
"""

# Standalone full-page template. Each page links the shared stylesheet via a
# depth-relative {css_href}; the browser caches it across demos.
PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · unxml</title>
<link rel="stylesheet" href="{css_href}">
</head>
<body>
<a class="unxml-back" href="{back}">← all demos</a>
<pre class="unxml"><span class="ansi2html-content">{fragment}</span></pre>
</body>
</html>
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


def index_markdown(entries: list[tuple[str, str, str, int]]) -> str:
    out = [
        "# unxml demos\n",
        "Real-world XML Schemas rendered with [`unxml --xsd`]"
        "(https://github.com/vivainio/unxml-rs), syntax-highlighted with the "
        "same grammar `unxml` ships for `bat`. Each link opens the full "
        "rendered output edge-to-edge.\n",
        "| Schema | Lines | Source |",
        "| --- | --- | --- |",
    ]
    for href, title, source, lines in entries:
        src = f"[xsd]({source})" if source else "—"
        out.append(f"| [{title}]({href}) | {lines} | {src} |")
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "target", nargs="?", default=str(REPO_ROOT.parent / "unxml-demos"),
        help="path to the unxml-demos repo (default: ../unxml-demos)",
    )
    ap.add_argument(
        "--regen-css", action="store_true",
        help="regenerate the shared ansi.css palette (only needed when the bat "
             "theme, the unxml grammar, or bat itself changes)",
    )
    args = ap.parse_args()

    site = Path(args.target).resolve()
    docs = site / "docs"
    if not docs.is_dir():
        sys.exit(f"{docs} not found — is {site} the unxml-demos repo?")

    bat = find_bat()
    # One converter for the whole run so a regenerated palette covers every
    # colour class seen across all files.
    conv = Ansi2HTMLConverter(inline=False, dark_bg=True, scheme="xterm")

    demos_dir = docs / "demos"
    demos_dir.mkdir(parents=True, exist_ok=True)
    css_path = demos_dir / "ansi.css"

    # Clear previously generated pages, but keep the committed, created-once
    # ansi.css (the colour palette is fixed by the bat theme x grammar, not by
    # content — see --regen-css).
    for stale in demos_dir.rglob("*.html"):
        stale.unlink()
    (demos_dir / "index.md").unlink(missing_ok=True)

    # First pass: highlight everything (also primes the converter's palette).
    rendered: list[tuple[Path, str, str, str, int]] = []  # path, slug, title, fragment, lines
    for path in discover():
        rel = path.relative_to(DEMO_DIR).as_posix()
        fragment = conv.convert(highlight(bat, path), full=False)
        lines = path.read_text(encoding="utf-8").count("\n")
        # demo/ubl/foo.xsd.unxml -> demos/ubl/foo.html
        slug = rel.removesuffix(".unxml").replace(".xsd", "")
        title = SOURCES.get(rel, (Path(rel).stem, None))[0]
        rendered.append((path, slug, title, fragment, lines))

    # Created-once shared stylesheet: ansi2html's colour palette plus the page
    # chrome. Only (re)written when missing or explicitly requested.
    if args.regen_css or not css_path.exists():
        existed = css_path.exists()
        headers = conv.produce_headers().replace('<style type="text/css">', "").replace("</style>", "")
        # produce_headers() emits one rule line per span *occurrence*, so the
        # same ~260 colour rules repeat thousands of times. Dedupe by line
        # (order-preserving) to collapse it to the actual palette.
        seen: set[str] = set()
        palette = "\n".join(
            ln for ln in headers.splitlines()
            if ln.strip() and not (ln in seen or seen.add(ln))
        )
        css_path.write_text(palette + "\n" + CHROME_CSS, encoding="utf-8")
        print(f"  {'regenerated' if existed else 'generated'} {css_path.relative_to(site)}")
    else:
        # Tripwire: warn if any page references a class the committed css lacks
        # (i.e. the tooling changed and the palette is stale).
        defined = set(re.findall(r"\.([A-Za-z0-9_-]+)", css_path.read_text(encoding="utf-8")))
        used = {c for _, _, _, frag, _ in rendered
                for attr in re.findall(r'class="([^"]+)"', frag) for c in attr.split()}
        if missing := used - defined:
            print(f"  WARNING: {css_path.name} is missing {len(missing)} class(es) "
                  f"e.g. {sorted(missing)[:3]} — rerun with --regen-css")

    # Write each standalone full-page document linking the shared css.
    entries: list[tuple[str, str, str, int]] = []  # href, title, source, lines
    for path, slug, title, fragment, lines in rendered:
        out_path = demos_dir / f"{slug}.html"
        out_path.parent.mkdir(parents=True, exist_ok=True)
        # Path back up to demos/ (where the index and ansi.css live) from this
        # page's nesting depth: "./" at the top, "../" per level down.
        back = "../" * slug.count("/") or "./"
        out_path.write_text(
            PAGE.format(title=title, css_href=f"{back}ansi.css", fragment=fragment, back=back),
            encoding="utf-8",
        )
        rel = path.relative_to(DEMO_DIR).as_posix()
        source = SOURCES.get(rel, (None, None))[1]
        entries.append((f"{slug}.html", title, source, lines))
        print(f"  rendered {rel} -> {out_path.relative_to(site)} ({lines} lines)")

    (demos_dir / "index.md").write_text(index_markdown(entries), encoding="utf-8")
    print(f"  wrote index -> {(demos_dir / 'index.md').relative_to(site)}")
    print(f"\nDone. {len(entries)} full-page demos written to {demos_dir.relative_to(site)}/.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
