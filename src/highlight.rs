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

/// Render `body` (already-processed `unxml` output) as a standalone HTML
/// page with classed spans. With `embed_css`, the stylesheet `--html-css`
/// would otherwise produce is inlined in a `<style>` block instead of
/// linked as `unxml.css`, so the page has no sibling file to keep with it.
pub(crate) fn html_page(body: &str, embed_css: bool) -> Result<String> {
    let syntax_set = syntax_set()?;
    let syntax = syntax_set
        .find_syntax_by_name("UnXML")
        .context("Bundled grammar does not define an 'UnXML' syntax")?;

    let mut generator =
        ClassedHTMLGenerator::new_with_class_style(syntax, &syntax_set, ClassStyle::Spaced);
    for line in LinesWithEndings::from(body) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .context("Failed to syntax-highlight the rendered output")?;
    }
    let spans = generator.finalize();

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

/// Render `body` as ANSI-escaped text for a terminal: same bundled
/// grammar/theme as `--html`, but escaped straight to stdout with no pager
/// and no external `bat`/`batcat` process — just `cat`, in color.
pub(crate) fn ansi(body: &str) -> Result<String> {
    let syntax_set = syntax_set()?;
    let syntax = syntax_set
        .find_syntax_by_name("UnXML")
        .context("Bundled grammar does not define an 'UnXML' syntax")?;
    let theme = &ThemeSet::load_defaults().themes[THEME_NAME];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut out = String::new();
    for line in LinesWithEndings::from(body) {
        let ranges = highlighter
            .highlight_line(line, &syntax_set)
            .context("Failed to syntax-highlight the rendered output")?;
        // No background escapes (bat doesn't paint full-width either); reset
        // at the very end so color never leaks into the shell prompt.
        out.push_str(&as_24_bit_terminal_escaped(&ranges[..], false));
    }
    out.push_str("\x1b[0m");
    Ok(out)
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
