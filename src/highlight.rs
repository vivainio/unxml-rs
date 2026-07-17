//! `--html` / `--html-css`: render output as syntax-highlighted HTML using
//! `syntect` and the same bundled Sublime grammar `--install-bat` registers
//! with `bat` — no external `bat` or Python step required.
//!
//! HTML gets classed spans (`ClassStyle::Spaced`) rather than inline
//! `style="..."` colors, so a single `--html-css` stylesheet can be shared
//! across every `--html` page instead of being duplicated into each one.

use anyhow::{Context, Result};
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::{SyntaxDefinition, SyntaxSetBuilder};
use syntect::util::LinesWithEndings;

use crate::install::BAT_SYNTAX;

/// One of syntect's bundled themes; only its color assignments per scope are
/// used (via `--html-css`), not anything shipped by `bat` itself.
const THEME_NAME: &str = "base16-ocean.dark";

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
/// page with classed spans, linking the stylesheet `--html-css` produces.
pub(crate) fn html_page(body: &str) -> Result<String> {
    let mut builder = SyntaxSetBuilder::new();
    builder.add(
        SyntaxDefinition::load_from_str(BAT_SYNTAX, true, None)
            .context("Failed to parse the bundled unxml.sublime-syntax grammar")?,
    );
    let syntax_set = builder.build();
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

    Ok(format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>unxml</title>\n\
         <link rel=\"stylesheet\" href=\"unxml.css\">\n\
         </head>\n\
         <body>\n\
         <pre class=\"unxml\">{spans}</pre>\n\
         </body>\n\
         </html>\n"
    ))
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
