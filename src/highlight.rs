//! `--html` / `--html-css` / `--cat`: render output as syntax-highlighted
//! HTML or ANSI-escaped terminal text using `syntect` and the same bundled
//! Sublime grammar `--install-bat` registers with `bat` — no external `bat`
//! or Python step required.
//!
//! HTML gets classed spans (`ClassStyle::Spaced`) rather than inline
//! `style="..."` colors, so a single `--html-css` stylesheet can be shared
//! across every `--html` page instead of being duplicated into each one.

use anyhow::{Context, Result};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};
use syntect::util::{LinesWithEndings, as_24_bit_terminal_escaped};

use crate::install::BAT_SYNTAX;

/// One of syntect's bundled themes; only its color assignments per scope are
/// used (via `--html-css`), not anything shipped by `bat` itself.
const THEME_NAME: &str = "base16-ocean.dark";

/// Build a `SyntaxSet` containing just the bundled `unxml` grammar. Shared by
/// `html_page` and `ansi` — each still looks up "UnXML" itself since a
/// `SyntaxReference` borrows from the set that produced it.
fn syntax_set() -> Result<SyntaxSet> {
    let mut builder = SyntaxSetBuilder::new();
    builder.add(
        SyntaxDefinition::load_from_str(BAT_SYNTAX, true, None)
            .context("Failed to parse the bundled unxml.sublime-syntax grammar")?,
    );
    Ok(builder.build())
}

/// Syntect's own bundled syntax set (the `default-syntaxes` feature already
/// pulled in by `default-fancy`), which happens to include general-purpose
/// "XML" and "HTML" grammars alongside hundreds of others never referenced
/// here. Used only by `--raw`, to highlight the original, untransformed
/// source next to unxml's output — no `bat`/`ansi2html` round-trip needed.
fn raw_syntax_set() -> SyntaxSet {
    SyntaxSet::load_defaults_newlines()
}

/// Look up `name` ("UnXML", "XML", or "HTML") in `syntax_set`, for callers
/// that already picked which grammar they want.
fn find_syntax<'a>(
    syntax_set: &'a SyntaxSet,
    name: &str,
) -> Result<&'a syntect::parsing::SyntaxReference> {
    syntax_set
        .find_syntax_by_name(name)
        .with_context(|| format!("Bundled syntax set does not define a '{name}' syntax"))
}

/// Page chrome shared by every `--html` render: dark background, monospace
/// font, and the `pre.unxml` white-space handling the highlighted output
/// needs. Kept separate from the theme's own token-color rules so it never
/// has to be regenerated when the theme does.
const CHROME_CSS: &str = "
html, body {
  margin: 0;
  background: #0d1117;
}
pre.unxml {
  margin: 0;
  padding: 1.25rem 1.5rem;
  color: #c9d1d9;
  font: 13px/1.5 ui-monospace, \"SF Mono\", SFMono-Regular, Menlo, Consolas, monospace;
  white-space: pre;
  tab-size: 2;
  overflow-x: auto;
}
";

/// Highlight `body` through `syntax` into classed HTML spans. Shared by the
/// unxml-grammar and raw-XML/HTML paths, which differ only in which
/// `SyntaxSet`/`SyntaxReference` they look up.
fn highlight_spans(
    syntax_set: &SyntaxSet,
    syntax: &syntect::parsing::SyntaxReference,
    body: &str,
) -> Result<String> {
    let mut generator =
        ClassedHTMLGenerator::new_with_class_style(syntax, syntax_set, ClassStyle::Spaced);
    for line in LinesWithEndings::from(body) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .context("Failed to syntax-highlight the source")?;
    }
    Ok(generator.finalize())
}

/// Wrap already-highlighted `spans` in the standalone page chrome shared by
/// `html_page` and `html_page_raw`.
fn page(spans: &str, embed_css: bool) -> Result<String> {
    let stylesheet = if embed_css {
        format!("<style>\n{}</style>", html_css()?)
    } else {
        "<link rel=\"stylesheet\" href=\"unxml.css\">".to_string()
    };

    Ok(format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>unxml</title>\n\
         {stylesheet}\n\
         </head>\n\
         <body>\n\
         <pre class=\"unxml\">{spans}</pre>\n\
         </body>\n\
         </html>\n"
    ))
}

/// Render `body` (already-processed `unxml` output) as a standalone HTML
/// page with classed spans. With `embed_css`, the stylesheet `--html-css`
/// would otherwise produce is inlined in a `<style>` block instead of
/// linked as `unxml.css`, so the page has no sibling file to keep with it.
pub(crate) fn html_page(body: &str, embed_css: bool) -> Result<String> {
    let syntax_set = syntax_set()?;
    let syntax = find_syntax(&syntax_set, "UnXML")?;
    page(&highlight_spans(&syntax_set, syntax, body)?, embed_css)
}

/// Like `html_page`, but for `--raw`: highlights `source` as-is (no unxml
/// transform) using syntect's bundled XML or HTML grammar instead of the
/// unxml one. Shares the same `unxml.css`/`--html-css` palette, since that
/// stylesheet is keyed by the theme's generic TextMate scope names
/// (comment, string, keyword, entity.name.tag, ...) rather than anything
/// specific to the unxml grammar.
pub(crate) fn html_page_raw(source: &str, is_html: bool, embed_css: bool) -> Result<String> {
    let syntax_set = raw_syntax_set();
    let syntax = find_syntax(&syntax_set, if is_html { "HTML" } else { "XML" })?;
    page(&highlight_spans(&syntax_set, syntax, source)?, embed_css)
}

/// Highlight `body` through `syntax` as ANSI-escaped text for a terminal.
/// Shared by `ansi` and `ansi_raw`.
fn highlight_ansi(
    syntax_set: &SyntaxSet,
    syntax: &syntect::parsing::SyntaxReference,
    body: &str,
) -> Result<String> {
    let theme = &ThemeSet::load_defaults().themes[THEME_NAME];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut out = String::new();
    for line in LinesWithEndings::from(body) {
        let ranges = highlighter
            .highlight_line(line, syntax_set)
            .context("Failed to syntax-highlight the source")?;
        // No background escapes (bat doesn't paint full-width either); reset
        // at the very end so color never leaks into the shell prompt.
        out.push_str(&as_24_bit_terminal_escaped(&ranges[..], false));
    }
    out.push_str("\x1b[0m");
    Ok(out)
}

/// Render `body` as ANSI-escaped text for a terminal: same bundled
/// grammar/theme as `--html`, but escaped straight to stdout with no pager
/// and no external `bat`/`batcat` process — just `cat`, in color.
pub(crate) fn ansi(body: &str) -> Result<String> {
    let syntax_set = syntax_set()?;
    let syntax = find_syntax(&syntax_set, "UnXML")?;
    highlight_ansi(&syntax_set, syntax, body)
}

/// Like `ansi`, but for `--raw`: highlights `source` as-is using syntect's
/// bundled XML or HTML grammar instead of the unxml one.
pub(crate) fn ansi_raw(source: &str, is_html: bool) -> Result<String> {
    let syntax_set = raw_syntax_set();
    let syntax = find_syntax(&syntax_set, if is_html { "HTML" } else { "XML" })?;
    highlight_ansi(&syntax_set, syntax, source)
}

/// The stylesheet every `--html` page links as `unxml.css`: per-scope token
/// colors from the bundled theme, plus page chrome. Save it once
/// (`unxml --html-css > unxml.css`) alongside any number of `--html` pages —
/// it only needs regenerating if the bundled theme changes.
pub(crate) fn html_css() -> Result<String> {
    let theme = &ThemeSet::load_defaults().themes[THEME_NAME];
    let palette = css_for_theme_with_class_style(theme, ClassStyle::Spaced)
        .context("Failed to generate CSS for the bundled highlighting theme")?;
    Ok(format!("{palette}\n{CHROME_CSS}"))
}
