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
UBL_XML = "https://docs.oasis-open.org/ubl/os-UBL-2.1/xml"
DOCBOOK = "https://cdn.docbook.org/release/xsl/current"
# DocBook xslTNG: the modern, MIT-licensed XSLT 3.0 reimplementation of the
# DocBook stylesheets. Real-world code exercising xsl:function, maps, xsl:iterate,
# xsl:try/catch and friends — a counterpoint to the classic XSLT 1.0 DOCBOOK above.
XSLTNG = "https://raw.githubusercontent.com/docbook/xslTNG/main/src/main/xslt"
SCHEMATRON = "https://raw.githubusercontent.com/Schematron/schematron/master/trunk/schematron/code"
EN16931 = ("https://raw.githubusercontent.com/ConnectingEurope/"
           "eInvoicing-EN16931/master/ubl/schematron")
# UN/CEFACT Cross Industry Invoice (CII) instance samples. phax's en16931-cii2ubl
# carries a canonical EN16931 CII example; mustangproject carries Factur-X /
# ZUGFeRD profile samples (both MIT-licensed, with clear provenance).
CII_PHAX = ("https://raw.githubusercontent.com/phax/en16931-cii2ubl/master/"
            "en16931-cii2ubl-cli/src/test/resources")
MUSTANG = ("https://raw.githubusercontent.com/ZUGFeRD/mustangproject/master/"
           "library/src/test/resources")

# The single source of truth: (unxml mode, output slug, title, source URL).
# Mode picks both the `unxml --<mode>` flag and the category subdir/section.
# "auto" renders an instance document in plain mode; for known vocabularies
# (e.g. UBL) it also sniffs and hides the noisy namespace prefixes.
DEMOS: list[tuple[str, str, str, str]] = [
    ("auto", "ubl/invoice-example", "UBL — Invoice (instance)", f"{UBL_XML}/UBL-Invoice-2.1-Example.xml"),
    ("auto", "ubl/order-example", "UBL — Order (instance)", f"{UBL_XML}/UBL-Order-2.1-Example.xml"),
    ("auto", "ubl/creditnote-example", "UBL — Credit Note (instance)", f"{UBL_XML}/UBL-CreditNote-2.1-Example.xml"),
    ("auto", "cii/invoice-example", "CII — Invoice (instance)", f"{CII_PHAX}/CII_example1.xml"),
    ("auto", "cii/factur-x-extended", "Factur-X / ZUGFeRD — Extended (instance)", f"{MUSTANG}/factur-x-extended.xml"),
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
    ("xslt", "docbook/xsltng-functions", "DocBook xslTNG 3.0 — function library", f"{XSLTNG}/modules/functions.xsl"),
    ("xslt", "docbook/xsltng-variable", "DocBook xslTNG 3.0 — variables & maps", f"{XSLTNG}/modules/variable.xsl"),
    ("xslt", "docbook/xsltng-l10n", "DocBook xslTNG 3.0 — localization (try/catch, iterate)", f"{XSLTNG}/modules/l10n.xsl"),
    ("xslt", "docbook/xsltng-profile", "DocBook xslTNG 3.0 — profiling transform", f"{XSLTNG}/transforms/40-profile.xsl"),
    ("xslt", "schematron/iso-svrl", "ISO Schematron — SVRL skeleton", f"{SCHEMATRON}/iso_svrl_for_xslt1.xsl"),
    ("schematron", "en16931/ubl-validation", "EN16931 — UBL validation (driver)", f"{EN16931}/EN16931-UBL-validation.sch"),
    ("schematron", "en16931/model", "EN16931 — abstract model rules", f"{EN16931}/abstract/EN16931-model.sch"),
    ("schematron", "en16931/ubl-model", "EN16931 — UBL bindings", f"{EN16931}/UBL/EN16931-UBL-model.sch"),
]
# mode -> (subdir, index-section heading); SECTION_ORDER sets section order.
MODE_CATEGORY = {
    "auto": ("xml", "XML documents"),
    "xsd": ("schemas", "Schemas"),
    "xslt": ("xslt", "XSLT"),
    "schematron": ("schematron", "Schematron"),
}
SECTION_ORDER = ["XML documents", "Schemas", "XSLT", "Schematron"]

# Small, self-hosted samples rendered *inline* on the gallery page — source
# beside output, so the transformation is visible at a glance without opening a
# full page. Sources are vendored in this (the unxml-demos) repo under
# examples/, so the site doesn't depend on a third-party host for them.
# tuple: (section heading, unxml mode, title, repo-relative source path)
INLINE_DEMOS: list[tuple[str, str, str, str]] = [
    ("Invoice basics", "auto", "CII / Factur-X — minimal invoice", "examples/cii/factur-x-basic.xml"),
    ("Folding boilerplate", "auto", "UBL — ext:UBLExtensions collapsed under --auto", "examples/ubl/invoice-with-extensions.xml"),
    ("XSLT basics", "xslt", "Build an HTML table with for-each", "examples/xslt/cdcatalog.xsl"),
    ("XSLT basics", "xslt", "Branch with choose / when / otherwise", "examples/xslt/cdcatalog-choose.xsl"),
    ("XSLT basics", "xslt", "Named templates + apply-templates", "examples/xslt/cdcatalog-templates.xsl"),
    ("XSLT basics", "xslt", "Literal-result-element stylesheet", "examples/xslt/breakfast-menu.xsl"),
]
INLINE_ORDER = ["Invoice basics", "Folding boilerplate", "XSLT basics"]
# Per-mode label for the left ("source") column of an inline side-by-side sample.
SOURCE_LABEL = {"xslt": "XSLT source", "auto": "XML source"}
SITE_REPO_BLOB = "https://github.com/vivainio/unxml-demos/blob/main"

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

# Layout for the inline side-by-side samples on the (light-themed) gallery
# page. The ansi2html colour palette is scoped under `.unxml-demo` and injected
# alongside this so it can't bleed into the surrounding theme.
INLINE_CSS = """
.unxml-demo { margin: 1rem 0 0; }
.unxml-demo .unxml-sample { margin: 0 0 1.75rem; }
.unxml-demo h3 { margin: 0 0 .15rem; }
.unxml-demo .unxml-cap { margin: 0 0 .5rem; font-size: .8em; color: #8b949e; }
.unxml-demo .unxml-cap a { color: inherit; text-decoration: underline; }
.unxml-demo .unxml-cols {
  display: grid; grid-template-columns: 1fr 1fr; gap: .75rem; align-items: start;
}
.unxml-demo .unxml-col { min-width: 0; }
.unxml-demo .unxml-col-label {
  font: 12px/1.4 ui-monospace, monospace; color: #8b949e; margin: 0 0 .25rem;
}
.unxml-demo pre.unxml {
  margin: 0; padding: .9rem 1rem; border-radius: 6px;
  background: #0d1117; color: #c9d1d9;
  font: 12px/1.5 ui-monospace, "SF Mono", SFMono-Regular, Menlo, Consolas, monospace;
  white-space: pre; tab-size: 2; overflow-x: auto; max-width: 100%;
}
@media (max-width: 820px) {
  .unxml-demo .unxml-cols { grid-template-columns: 1fr; }
}
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


def highlight(bat: str, text: str, lang: str = "unxml") -> str:
    """ANSI-highlight text via bat. Defaults to the unxml grammar; pass e.g.
    lang="xml" to highlight an original source document."""
    return subprocess.run(
        [bat, "--color=always", "--paging=never", "--style=plain",
         "--wrap=never", "-l", lang],
        input=text, capture_output=True, text=True, check=True,
    ).stdout


def compact(text: str) -> str:
    """Drop blank/whitespace-only lines so a highlighted fragment can live in a
    raw-HTML block inside Markdown without a blank line ending the block."""
    return "\n".join(ln for ln in text.splitlines() if ln.strip())


def scope_css(css: str, prefix: str) -> str:
    """Prefix every rule's selectors with `prefix` so a flat stylesheet (e.g.
    ansi2html's colour palette) can't leak into the surrounding page."""
    out = []
    for line in css.splitlines():
        s = line.strip()
        if not s or s.startswith("@") or "{" not in s:
            out.append(line)
            continue
        sels, _, rest = s.partition("{")
        scoped = ", ".join(f"{prefix} {sel.strip()}" for sel in sels.split(","))
        out.append(f"{scoped} {{{rest}")
    return "\n".join(out)


def human_bytes(n: int) -> str:
    """Compact size label: B / KB / MB with one decimal where it helps."""
    if n < 1024:
        return f"{n} B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f} KB"
    return f"{n / (1024 * 1024):.1f} MB"


def inline_section_html(samples: list[tuple]) -> str:
    """One raw-HTML block for a group of inline side-by-side samples. Kept free
    of blank lines so Markdown passes it through verbatim."""
    html = ['<div class="unxml-demo">']
    for _section, mode, title, blob, src_frag, out_frag, sl, ol in samples:
        fname = blob.rsplit("/", 1)[-1]
        html += [
            '<div class="unxml-sample">',
            f"<h3>{title}</h3>",
            f'<p class="unxml-cap"><a href="{blob}">{fname}</a> · '
            f"{sl} → {ol} lines</p>",
            '<div class="unxml-cols">',
            f'<div class="unxml-col"><div class="unxml-col-label">{SOURCE_LABEL.get(mode, "source")}</div>'
            f'<pre class="unxml"><span class="ansi2html-content">{src_frag}</span></pre></div>',
            f'<div class="unxml-col"><div class="unxml-col-label">unxml --{mode}</div>'
            f'<pre class="unxml"><span class="ansi2html-content">{out_frag}</span></pre></div>',
            "</div></div>",
        ]
    html.append("</div>")
    return "\n".join(html)


def index_markdown(
    entries: list[tuple[str, str, str, str, int, int, int, int]],
    inline: list[tuple],
    inline_style: str,
) -> str:
    # entries: (heading, href, title, source, src_lines, src_bytes, out_lines, out_bytes)
    # inline: (section, mode, title, blob_url, src_frag, out_frag, src_lines, out_lines)
    # Each element of `blocks` is a complete block with no internal blank lines;
    # joining with a blank line keeps tables and raw-HTML blocks intact.
    blocks = [
        # Hide the right-hand "table of contents" sidebar on the gallery: its
        # headings are just section names and the extra column steals width the
        # side-by-side demos need.
        "---\nhide:\n  - toc\n---",
        "# Gallery",
        "Real-world XML documents rendered with [`unxml`]"
        "(https://github.com/vivainio/unxml-rs), syntax-highlighted with the "
        "same grammar `unxml` ships for `bat`.",
    ]

    if inline:
        blocks.append(
            "The **basics** below are shown inline — original source on the "
            "left, `unxml` output on the right. The **gallery** further down "
            "links full-page renders of larger real-world documents, with "
            "original-vs-rendered size comparisons."
        )
        blocks.append(inline_style)
        for section in INLINE_ORDER:
            samples = [s for s in inline if s[0] == section]
            if not samples:
                continue
            blocks.append(f"## {section}")
            blocks.append(inline_section_html(samples))
    else:
        blocks.append(
            "The **Original** and **Rendered** columns compare the source XML "
            "against the `unxml` output (lines · bytes)."
        )

    for heading in SECTION_ORDER:
        rows = [e for e in entries if e[0] == heading]
        if not rows:
            continue
        table = [
            f"## {heading}",
            "| Document | Original | Rendered | Source |",
            "| --- | --- | --- | --- |",
        ]
        for _, href, title, source, sl, sb, ol, ob in rows:
            original = f"{sl:,} lines · {human_bytes(sb)}"
            rendered = f"{ol:,} lines · {human_bytes(ob)}"
            table.append(
                f"| [{title}]({href}) | {original} | {rendered} | [source]({source}) |"
            )
        blocks.append("\n".join(table))

    return "\n\n".join(blocks) + "\n"


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
    # tuple: out_slug, heading, title, url, fragment, src_lines, src_bytes, out_lines, out_bytes
    rendered: list[tuple[str, str, str, str, str, int, int, int, int]] = []
    for mode, slug, title, url in DEMOS:
        subdir, heading = MODE_CATEGORY[mode]
        ext = Path(urlsplit(url).path).suffix or f".{mode}"
        cache_path = CACHE_DIR / f"{slug}{ext}"
        was_cached = cache_path.exists()
        if not fetch(url, cache_path):
            continue
        src_bytes = cache_path.read_bytes()
        src_lines = src_bytes.count(b"\n")
        text = render(mode, cache_path)
        fragment = conv.convert(highlight(bat, text), full=False)
        out_slug = f"{subdir}/{slug}"
        out_lines = text.count("\n")
        out_bytes = len(text.encode("utf-8"))
        rendered.append((out_slug, heading, title, url, fragment,
                         src_lines, len(src_bytes), out_lines, out_bytes))
        print(f"  {'cached' if was_cached else 'fetched'} + rendered {slug} "
              f"({src_lines}->{out_lines} lines)")

    # Inline samples: render source + output (both highlighted) for the
    # side-by-side gallery section. Sources are vendored in the site repo, so
    # this also primes the palette with the XML highlighting classes.
    # tuple: section, mode, title, blob_url, src_frag, out_frag, src_lines, out_lines
    inline_rendered: list[tuple] = []
    for section, mode, title, rel in INLINE_DEMOS:
        src_path = site / rel
        if not src_path.exists():
            print(f"  WARNING: inline source {rel} not found under {site}")
            continue
        src_text = src_path.read_text(encoding="utf-8")
        out_text = render(mode, src_path)
        src_frag = conv.convert(highlight(bat, compact(src_text), lang="xml"), full=False)
        out_frag = conv.convert(highlight(bat, compact(out_text)), full=False)
        inline_rendered.append((
            section, mode, title, f"{SITE_REPO_BLOB}/{rel}", src_frag, out_frag,
            src_text.count("\n"), out_text.count("\n"),
        ))
        print(f"  inline + rendered {rel} "
              f"({src_text.count(chr(10))}->{out_text.count(chr(10))} lines)")

    # Colour palette from ansi2html (deduped to the ~260-rule set), shared by
    # the full-page ansi.css and the scoped inline <style>. produce_headers()
    # emits one rule line per span *occurrence*; dedupe order-preserving.
    headers = conv.produce_headers().replace('<style type="text/css">', "").replace("</style>", "")
    seen: set[str] = set()
    palette = "\n".join(
        ln for ln in headers.splitlines()
        if ln.strip() and not (ln in seen or seen.add(ln))
    )

    # Created-once shared stylesheet for full-page demos: palette plus chrome.
    if args.regen_css or not css_path.exists():
        existed = css_path.exists()
        css_path.write_text(palette + "\n" + CHROME_CSS, encoding="utf-8")
        print(f"  {'regenerated' if existed else 'generated'} {css_path.relative_to(site)}")
    else:
        # Tripwire: warn if a page references a class the committed css lacks.
        defined = set(re.findall(r"\.([A-Za-z0-9_-]+)", css_path.read_text(encoding="utf-8")))
        used = {c for r in rendered
                for attr in re.findall(r'class="([^"]+)"', r[4]) for c in attr.split()}
        if missing := used - defined:
            print(f"  WARNING: {css_path.name} is missing {len(missing)} class(es) "
                  f"e.g. {sorted(missing)[:3]} — rerun with --regen-css")

    # Second pass: write each standalone full-page document linking the css.
    # entries: heading, href, title, source, src_lines, src_bytes, out_lines, out_bytes
    entries: list[tuple[str, str, str, str, int, int, int, int]] = []
    for out_slug, heading, title, url, fragment, sl, sb, ol, ob in rendered:
        out_path = demos_dir / f"{out_slug}.html"
        out_path.parent.mkdir(parents=True, exist_ok=True)
        # "../" per level below demos/ (where the index and ansi.css live).
        back = "../" * out_slug.count("/") or "./"
        out_path.write_text(
            PAGE.format(title=title, css_href=f"{back}ansi.css", fragment=fragment, back=back),
            encoding="utf-8",
        )
        entries.append((heading, f"{out_slug}.html", title, url, sl, sb, ol, ob))

    # Scoped palette + layout for the inline side-by-side samples, injected as
    # a self-contained <style> so it can't bleed into the surrounding theme.
    inline_style = ""
    if inline_rendered:
        inline_style = ("<style>\n" + scope_css(palette, ".unxml-demo").strip()
                        + "\n" + INLINE_CSS.strip() + "\n</style>")

    (demos_dir / "index.md").write_text(
        index_markdown(entries, inline_rendered, inline_style), encoding="utf-8")
    print(f"\nDone. {len(entries)} full-page + {len(inline_rendered)} inline "
          f"demos written to {demos_dir.relative_to(site)}/.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
