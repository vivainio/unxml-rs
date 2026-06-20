#!/usr/bin/env python3
"""Render real-world XML schemas / stylesheets to syntax-highlighted, full-page
HTML for the Zensical demo site (https://github.com/vivainio/unxml-demos).

Fully fetch-driven: the DEMOS manifest below lists canonical source URLs.
Each source is downloaded into a gitignored cache (demo/.cache/) on demand,
rendered with the local `unxml` binary, highlighted by `bat` (using the
`unxml` grammar) piped to `ansi2html`, and written as a self-contained
full-page HTML document under docs/demos/<category>/ in the site repo. A
themed Markdown index links to them, grouped by category. Nothing large is
vendored — re-running fetches anything missing from the cache.

Requirements:
  - the release binary built: `cargo build --release` (in unxml-rs)
  - `bat` on PATH with the unxml grammar installed
    (run `python3 editor/install-editor-support.py` first)
  - `pip install ansi2html`
  - network access (first run, or after clearing the cache)

Usage:
  python3 demo/publish-to-demo-site.py [PATH_TO_unxml-demos] [--regen-css]

PATH defaults to ../unxml-demos relative to this repo.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import urllib.request
from pathlib import Path
from urllib.error import URLError
from urllib.parse import urlsplit

try:
    from ansi2html import Ansi2HTMLConverter
except ImportError:
    sys.exit("ansi2html not installed. Run: pip install ansi2html")

DEMO_DIR = Path(__file__).resolve().parent
REPO_ROOT = DEMO_DIR.parent
CACHE_DIR = DEMO_DIR / ".cache"  # gitignored; downloaded sources live here
UNXML_BIN = REPO_ROOT / "target" / "release" / (
    "unxml.exe" if sys.platform == "win32" else "unxml")

UBL = "https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd"
DOCBOOK = "https://cdn.docbook.org/release/xsl/current"
SCHEMATRON = "https://raw.githubusercontent.com/Schematron/schematron/master/trunk/schematron/code"

# The single source of truth: (unxml mode, output slug, title, source URL).
# Mode picks both the `unxml --<mode>` flag and the category subdir/section.
DEMOS: list[tuple[str, str, str, str]] = [
    ("xsd", "finvoice-3.0", "Finvoice 3.0", "https://file.finanssiala.fi/finvoice/Finvoice3.0.xsd"),
    ("xsd", "ubl/cct", "UBL — Core Component Types", f"{UBL}/common/CCTS_CCT_SchemaModule-2.1.xsd"),
    ("xsd", "ubl/udt", "UBL — Unqualified Data Types", f"{UBL}/common/UBL-UnqualifiedDataTypes-2.1.xsd"),
    ("xsd", "ubl/qdt", "UBL — Qualified Data Types", f"{UBL}/common/UBL-QualifiedDataTypes-2.1.xsd"),
    ("xsd", "ubl/cbc", "UBL — Common Basic Components", f"{UBL}/common/UBL-CommonBasicComponents-2.1.xsd"),
    ("xsd", "ubl/cac", "UBL — Common Aggregate Components", f"{UBL}/common/UBL-CommonAggregateComponents-2.1.xsd"),
    ("xsd", "ubl/cec", "UBL — Common Extension Components", f"{UBL}/common/UBL-CommonExtensionComponents-2.1.xsd"),
    ("xsd", "ubl/invoice", "UBL — Invoice", f"{UBL}/maindoc/UBL-Invoice-2.1.xsd"),
    ("xslt", "docbook/html-driver", "DocBook XSL — HTML driver", f"{DOCBOOK}/html/docbook.xsl"),
    ("xslt", "docbook/inline", "DocBook XSL — inline elements", f"{DOCBOOK}/html/inline.xsl"),
    ("xslt", "schematron/iso-svrl", "ISO Schematron — SVRL skeleton", f"{SCHEMATRON}/iso_svrl_for_xslt1.xsl"),
]
# mode -> (subdir, index-section heading); SECTION_ORDER sets section order.
MODE_CATEGORY = {"xsd": ("schemas", "Schemas"), "xslt": ("xslt", "XSLT")}
SECTION_ORDER = ["Schemas", "XSLT"]

# Page chrome shared by every standalone demo: a dark, edge-to-edge,
# horizontally-scrolling code surface plus the floating "back" link. Appended
# to ansi2html's colour classes in one shared stylesheet (demos/ansi.css).
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

# Standalone full-page template; links the shared stylesheet via a depth-
# relative {css_href} so the browser caches it across demos.
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


def fetch(url: str, dest: Path) -> bool:
    """Download url to dest if not already cached. Returns False on failure."""
    if dest.exists():
        return True
    dest.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "unxml-demos"})
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            dest.write_bytes(resp.read())
        return True
    except (URLError, OSError) as e:
        print(f"  WARNING: failed to fetch {url}: {e}")
        return False


def render(mode: str, src: Path) -> str:
    """Run `unxml --<mode> src` and return the rendered .unxml text."""
    return subprocess.run(
        [str(UNXML_BIN), f"--{mode}", str(src)],
        capture_output=True, text=True, check=True,
    ).stdout


def highlight(bat: str, text: str) -> str:
    """ANSI-highlight rendered text via bat using the unxml grammar."""
    return subprocess.run(
        [bat, "--color=always", "--paging=never", "--style=plain",
         "--wrap=never", "-l", "unxml"],
        input=text, capture_output=True, text=True, check=True,
    ).stdout


def index_markdown(entries: list[tuple[str, str, str, str, int]]) -> str:
    # entries: (heading, href, title, source, lines)
    out = [
        "# unxml demos\n",
        "Real-world XML documents rendered with [`unxml`]"
        "(https://github.com/vivainio/unxml-rs), syntax-highlighted with the "
        "same grammar `unxml` ships for `bat`. Each link opens the full "
        "rendered output edge-to-edge.\n",
    ]
    for heading in SECTION_ORDER:
        rows = [e for e in entries if e[0] == heading]
        if not rows:
            continue
        out += [f"## {heading}\n", "| Document | Lines | Source |", "| --- | --- | --- |"]
        for _, href, title, source, lines in rows:
            out.append(f"| [{title}]({href}) | {lines} | [source]({source}) |")
        out.append("")
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
    if not UNXML_BIN.exists():
        sys.exit(f"{UNXML_BIN} not found — run `cargo build --release` first.")

    bat = find_bat()
    # One converter for the whole run so a regenerated palette covers every
    # colour class seen across all files.
    conv = Ansi2HTMLConverter(inline=False, dark_bg=True, scheme="xterm")

    demos_dir = docs / "demos"
    demos_dir.mkdir(parents=True, exist_ok=True)
    css_path = demos_dir / "ansi.css"

    # Clear previously generated pages, keeping the created-once ansi.css.
    for stale in demos_dir.rglob("*.html"):
        stale.unlink()
    (demos_dir / "index.md").unlink(missing_ok=True)

    # First pass: fetch + render each demo (also primes the converter palette).
    rendered: list[tuple[str, str, str, str, str, int]] = []  # out_slug, heading, title, url, fragment, lines
    for mode, slug, title, url in DEMOS:
        subdir, heading = MODE_CATEGORY[mode]
        ext = Path(urlsplit(url).path).suffix or f".{mode}"
        cache_path = CACHE_DIR / f"{slug}{ext}"
        was_cached = cache_path.exists()
        if not fetch(url, cache_path):
            continue
        text = render(mode, cache_path)
        fragment = conv.convert(highlight(bat, text), full=False)
        out_slug = f"{subdir}/{slug}"
        rendered.append((out_slug, heading, title, url, fragment, text.count("\n")))
        print(f"  {'cached' if was_cached else 'fetched'} + rendered {slug} ({text.count(chr(10))} lines)")

    # Created-once shared stylesheet: ansi2html's colour palette plus chrome.
    if args.regen_css or not css_path.exists():
        existed = css_path.exists()
        headers = conv.produce_headers().replace('<style type="text/css">', "").replace("</style>", "")
        # produce_headers() emits one rule line per span *occurrence*; dedupe by
        # line (order-preserving) to collapse it to the ~260-colour palette.
        seen: set[str] = set()
        palette = "\n".join(
            ln for ln in headers.splitlines()
            if ln.strip() and not (ln in seen or seen.add(ln))
        )
        css_path.write_text(palette + "\n" + CHROME_CSS, encoding="utf-8")
        print(f"  {'regenerated' if existed else 'generated'} {css_path.relative_to(site)}")
    else:
        # Tripwire: warn if a page references a class the committed css lacks.
        defined = set(re.findall(r"\.([A-Za-z0-9_-]+)", css_path.read_text(encoding="utf-8")))
        used = {c for *_, frag, _ in rendered
                for attr in re.findall(r'class="([^"]+)"', frag) for c in attr.split()}
        if missing := used - defined:
            print(f"  WARNING: {css_path.name} is missing {len(missing)} class(es) "
                  f"e.g. {sorted(missing)[:3]} — rerun with --regen-css")

    # Second pass: write each standalone full-page document linking the css.
    entries: list[tuple[str, str, str, str, int]] = []  # heading, href, title, source, lines
    for out_slug, heading, title, url, fragment, lines in rendered:
        out_path = demos_dir / f"{out_slug}.html"
        out_path.parent.mkdir(parents=True, exist_ok=True)
        # "../" per level below demos/ (where the index and ansi.css live).
        back = "../" * out_slug.count("/") or "./"
        out_path.write_text(
            PAGE.format(title=title, css_href=f"{back}ansi.css", fragment=fragment, back=back),
            encoding="utf-8",
        )
        entries.append((heading, f"{out_slug}.html", title, url, lines))

    (demos_dir / "index.md").write_text(index_markdown(entries), encoding="utf-8")
    print(f"\nDone. {len(entries)} full-page demos written to {demos_dir.relative_to(site)}/.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
