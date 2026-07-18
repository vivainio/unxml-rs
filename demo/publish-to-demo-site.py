#!/usr/bin/env python3
"""Render real-world XML schemas / stylesheets to syntax-highlighted, full-page
HTML for the Zensical demo site (https://github.com/vivainio/unxml-demos).

Fully fetch-driven: the DEMOS manifest below lists canonical source URLs.
Each source is downloaded into a gitignored cache (demo/.cache/) on demand
and rendered with the local `unxml` binary. Both the `unxml` output and the
original source shown beside it in the inline gallery are highlighted
natively via `unxml --html` (syntect, no external process) — the output
with the bundled `unxml` grammar, the source with `--raw`, which highlights
the file as-is (XML/HTML) instead of transforming it. Each full-page demo
is written as a self-contained HTML document under docs/demos/<category>/
in the site repo, and a themed Markdown index links to them, grouped by
category. Nothing large is vendored — re-running fetches anything missing
from the cache.

`bat`/`ansi2html` are no longer a hard dependency: the one remaining use is
an INLINE_COMPARE row with an empty flag list (a "bare unxml, no dialect"
column) — `--html` can't highlight that without also triggering the implied
--auto sniffing that comes with it (see the comment at that call site), so
that single case still shells out to `bat -l unxml` piped through
`ansi2html`. Both are imported/located lazily, only if such a row exists.

Requirements:
  - the release binary built: `cargo build --release` (in unxml-rs), with
    `--raw` support (native XML/HTML source highlighting)
  - network access (first run, or after clearing the cache)
  - only if INLINE_COMPARE has an empty-flags side: `bat` on PATH (built-in
    `xml` support only) and `pip install ansi2html`

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
    # Only required for INLINE_COMPARE's bare-unxml-grammar fallback (see
    # module docstring); checked lazily in main() so it's not a hard
    # dependency for everyone else.
    Ansi2HTMLConverter = None

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
# Real MSBuild project/import files from Microsoft's own build tooling —
# found by poking around a local `dotnet` SDK install, then pointed at their
# canonical upstream sources (both MIT-licensed) so nothing large is vendored.
MSBUILD = "https://raw.githubusercontent.com/dotnet/msbuild/main/src/Tasks"
NUGET_CLIENT = ("https://raw.githubusercontent.com/NuGet/NuGet.Client/dev/"
                "src/NuGet.Core/NuGet.Build.Tasks")
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
    ("msbuild", "csharp-targets", "MSBuild — C# language targets", f"{MSBUILD}/Microsoft.CSharp.CurrentVersion.targets"),
    ("msbuild", "nuget-restore-targets", "MSBuild — NuGet restore targets", f"{NUGET_CLIENT}/NuGet.targets"),
    ("msbuild", "common-targets", "MSBuild — common build targets", f"{MSBUILD}/Microsoft.Common.CurrentVersion.targets"),
]
# mode -> (subdir, index-section heading); SECTION_ORDER sets section order.
MODE_CATEGORY = {
    "auto": ("xml", "XML documents"),
    "xsd": ("schemas", "Schemas"),
    "xslt": ("xslt", "XSLT"),
    "schematron": ("schematron", "Schematron"),
    "msbuild": ("msbuild", "MSBuild"),
}
SECTION_ORDER = ["XML documents", "Schemas", "XSLT", "Schematron", "MSBuild"]

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

# Side-by-side comparisons of the SAME document rendered with two flag sets,
# both through unxml — used to show what an opt-in flag buys. Unlike the
# source-vs-output samples above, both columns are unxml output.
# tuple: (section heading, title, repo-relative source path, left flags, right flags)
INLINE_COMPARE: list[tuple[str, str, str, list[str], list[str]]] = [
    ("Folding boilerplate", "CII / Factur-X — what --auto does (hide prefixes + fold wrapper chains)",
     "examples/cii/factur-x-basic.xml", [], ["--auto"]),
]
# Per-mode label for the left ("source") column of an inline side-by-side sample.
SOURCE_LABEL = {"xslt": "XSLT source", "auto": "XML source"}
SITE_REPO_BLOB = "https://github.com/vivainio/unxml-demos/blob/main"

# Floating "back" link for every standalone demo page. The rest of the page
# chrome (dark background, monospace pre, horizontal scroll) now ships in
# `unxml --html-css` itself, so this is all the demo site adds on top.
CHROME_CSS = """
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
/* See CHROME_CSS above for why this override is needed. */
.unxml-demo pre.unxml .ansi2html-content { white-space: inherit; word-wrap: normal; }
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
<pre class="unxml">{fragment}</pre>
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


def render_args(flags: list[str], src: Path) -> str:
    """Run `unxml <flags...> src` and return the rendered .unxml text."""
    return subprocess.run(
        [str(UNXML_BIN), *flags, str(src)],
        capture_output=True, text=True, check=True,
    ).stdout


def render(mode: str, src: Path) -> str:
    """Run `unxml --<mode> src` and return the rendered .unxml text."""
    return render_args([f"--{mode}"], src)


_PRE_RE = re.compile(r'<pre class="unxml">(.*)</pre>\n</body>', re.DOTALL)


def render_html(flags: list[str], src: Path) -> str:
    """Run `unxml <flags...> --html src` and return the classed spans inside
    its `<pre class="unxml">`, dropping the standalone-page chrome around it."""
    page = subprocess.run(
        [str(UNXML_BIN), *flags, "--html", str(src)],
        capture_output=True, text=True, check=True,
    ).stdout
    match = _PRE_RE.search(page)
    if not match:
        sys.exit(f"unxml --html output for {src} did not contain the expected <pre> block")
    return match.group(1)


_TAG_RE = re.compile(r"<[^>]+>")


def compact_html(fragment: str) -> str:
    """Like compact(), but operates after highlighting: keeps only lines whose
    text content (HTML tags stripped) is non-blank."""
    return "\n".join(ln for ln in fragment.splitlines() if _TAG_RE.sub("", ln).strip())


def html_css() -> str:
    """Run `unxml --html-css` and return the bundled token-colour stylesheet."""
    return subprocess.run(
        [str(UNXML_BIN), "--html-css"], capture_output=True, text=True, check=True,
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
    for _section, title, blob, left_label, left_frag, right_label, right_frag, ll, rl in samples:
        fname = blob.rsplit("/", 1)[-1]
        html += [
            '<div class="unxml-sample">',
            f"<h3>{title}</h3>",
            f'<p class="unxml-cap"><a href="{blob}">{fname}</a> · '
            f"{ll} → {rl} lines</p>",
            '<div class="unxml-cols">',
            f'<div class="unxml-col"><div class="unxml-col-label">{left_label}</div>'
            f'<pre class="unxml"><span class="ansi2html-content">{left_frag}</span></pre></div>',
            f'<div class="unxml-col"><div class="unxml-col-label">{right_label}</div>'
            f'<pre class="unxml"><span class="ansi2html-content">{right_frag}</span></pre></div>',
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
    # inline: (section, title, blob_url, left_label, left_frag, right_label, right_frag, left_lines, right_lines)
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
        help="regenerate the shared ansi.css palette (only needed when unxml's "
             "bundled highlighting theme changes)",
    )
    args = ap.parse_args()

    site = Path(args.target).resolve()
    docs = site / "docs"
    if not docs.is_dir():
        sys.exit(f"{docs} not found — is {site} the unxml-demos repo?")
    if not UNXML_BIN.exists():
        sys.exit(f"{UNXML_BIN} not found — run `cargo build --release` first.")

    # bat/ansi2html are only needed for INLINE_COMPARE's bare-unxml-grammar
    # fallback (an empty flag list on either side — see module docstring).
    # Located/constructed lazily so the common case never needs either.
    needs_bat = any(not lflags or not rflags for _, _, _, lflags, rflags in INLINE_COMPARE)
    if needs_bat and Ansi2HTMLConverter is None:
        sys.exit(
            "ansi2html not installed, but INLINE_COMPARE has a bare-unxml-grammar "
            "row that needs it as a fallback. Run: pip install ansi2html"
        )
    bat = find_bat() if needs_bat else None
    # One converter for the whole run so a regenerated palette covers every
    # colour class seen across all files.
    conv = Ansi2HTMLConverter(inline=False, dark_bg=True, scheme="xterm") if needs_bat else None

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
        fragment = render_html([f"--{mode}"], cache_path)
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
    # tuple: section, title, blob_url, left_label, left_frag, right_label, right_frag, left_lines, right_lines
    inline_rendered: list[tuple] = []
    for section, mode, title, rel in INLINE_DEMOS:
        src_path = site / rel
        if not src_path.exists():
            print(f"  WARNING: inline source {rel} not found under {site}")
            continue
        src_text = src_path.read_text(encoding="utf-8")
        out_text = render(mode, src_path)
        src_frag = compact_html(render_html(["--raw"], src_path))
        out_frag = compact_html(render_html([f"--{mode}"], src_path))
        inline_rendered.append((
            section, title, f"{SITE_REPO_BLOB}/{rel}",
            SOURCE_LABEL.get(mode, "source"), src_frag, f"unxml --{mode}", out_frag,
            src_text.count("\n"), out_text.count("\n"),
        ))
        print(f"  inline + rendered {rel} "
              f"({src_text.count(chr(10))}->{out_text.count(chr(10))} lines)")

    # Two-flag comparisons: both columns are unxml output of the same source.
    for section, title, rel, lflags, rflags in INLINE_COMPARE:
        src_path = site / rel
        if not src_path.exists():
            print(f"  WARNING: inline source {rel} not found under {site}")
            continue
        left_text = render_args(lflags, src_path)
        right_text = render_args(rflags, src_path)
        # `--html` implies --auto-style namespace sniffing whenever no
        # explicit mode flag is already set (see cli.rs), so an empty flag
        # list can't be highlighted via --html without picking up sniffing
        # it shouldn't have. Fall back to bat/ansi2html for that case, which
        # highlights the already-rendered plain text verbatim.
        left_frag = (compact_html(render_html(lflags, src_path)) if lflags
                     else conv.convert(highlight(bat, compact(left_text)), full=False))
        right_frag = (compact_html(render_html(rflags, src_path)) if rflags
                      else conv.convert(highlight(bat, compact(right_text)), full=False))
        inline_rendered.append((
            section, title, f"{SITE_REPO_BLOB}/{rel}",
            " ".join(["unxml", *lflags]), left_frag,
            " ".join(["unxml", *rflags]), right_frag,
            left_text.count("\n"), right_text.count("\n"),
        ))
        print(f"  inline compare {rel} "
              f"({left_text.count(chr(10))}->{right_text.count(chr(10))} lines)")

    # Token-colour palette for unxml output, straight from the binary — it
    # already bundles page chrome (dark background, monospace pre) matching
    # what this script used to hand-roll for the ansi2html path.
    unxml_palette = html_css()

    # ansi2html's palette (deduped to the ~260-rule set), only built when the
    # bare-unxml-grammar fallback above actually ran; produce_headers() emits
    # one rule line per span *occurrence*, so dedupe order-preserving.
    ansi_palette = ""
    if conv is not None:
        headers = conv.produce_headers().replace('<style type="text/css">', "").replace("</style>", "")
        seen: set[str] = set()
        ansi_palette = "\n".join(
            ln for ln in headers.splitlines()
            if ln.strip() and not (ln in seen or seen.add(ln))
        )

    # Created-once shared stylesheet for full-page demos: palette plus chrome.
    # Full pages are pure unxml --html output now, so no ansi2html classes
    # belong here.
    if args.regen_css or not css_path.exists():
        existed = css_path.exists()
        css_path.write_text(unxml_palette + "\n" + CHROME_CSS, encoding="utf-8")
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

    # Scoped palette(s) + layout for the inline side-by-side samples,
    # injected as a self-contained <style> so nothing bleeds into the
    # surrounding theme. unxml's palette covers both output and (--raw)
    # source columns now; ansi2html's is only included when the bare-
    # unxml-grammar fallback actually ran. INLINE_CSS comes last so its
    # layout rules win the cascade over either palette's own chrome.
    inline_style = ""
    if inline_rendered:
        parts = [scope_css(unxml_palette, ".unxml-demo").strip()]
        if ansi_palette:
            parts.append(scope_css(ansi_palette, ".unxml-demo").strip())
        parts.append(INLINE_CSS.strip())
        inline_style = "<style>\n" + "\n".join(parts) + "\n</style>"

    (demos_dir / "index.md").write_text(
        index_markdown(entries, inline_rendered, inline_style), encoding="utf-8")
    print(f"\nDone. {len(entries)} full-page + {len(inline_rendered)} inline "
          f"demos written to {demos_dir.relative_to(site)}/.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
