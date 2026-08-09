//! Readability-oriented rendering for Leo (leo-editor) `.leo` outline files.
//!
//! A `.leo` file is XML with two flat sibling sections under `<leo_file>`:
//! `<vnodes>` holds the outline tree (`<v t="gnx">`, nested, each with a
//! `<vh>` headline child) and `<tnodes>` holds a flat list of body text
//! (`<t tx="gnx">...</t>`), keyed by the same gnx used in `<v t="...">`. This
//! module joins the two into a single readable outline: each headline on its
//! own `* ` line, its body (if any) as an indented `|` block directly below —
//! never appended inline, since a headline may itself legitimately contain
//! `" = "` (e.g. Leo's own `@bool foo = True` settings headlines), which
//! would make an inline `headline = body` line ambiguous.
//!
//! A node cloned to more than one outline position is written by Leo with its
//! full `<vh>`/children only at the first occurrence; every later occurrence
//! is an empty `<v t="gnx"></v>` stub. Those stubs are rendered as
//! `* <headline> (clone)` with no body/children repeated, using the headline
//! recorded from that node's first, full occurrence. Everything else in the
//! file — gnx ids, expansion/mark state, `<leo_header>`/`<globals>`/
//! `<preferences>` bookkeeping — is dropped.

use std::collections::HashMap;

use crate::model::XmlElement;
use crate::xslt::TemplateRegistry;

impl XmlElement {
    pub(crate) fn format_leo_element(
        &self,
        indent: usize,
        _indent_str: &str,
        _registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        if self.name != "leo_file" {
            return None;
        }
        let vnodes = self.children.iter().find(|c| c.name == "vnodes")?;
        let tnodes = self.children.iter().find(|c| c.name == "tnodes");

        let bodies: HashMap<&str, &str> = tnodes
            .into_iter()
            .flat_map(|t| t.children.iter())
            .filter(|t| t.name == "t")
            .filter_map(|t| {
                t.attributes
                    .get("tx")
                    .map(|tx| (tx.as_str(), t.text_content.as_str()))
            })
            .collect();

        let mut headlines: HashMap<&str, &str> = HashMap::new();
        let mut result = String::new();
        for v in vnodes.children.iter().filter(|c| c.name == "v") {
            render_vnode(v, indent, &bodies, &mut headlines, &mut result);
        }
        Some(result)
    }
}

fn render_vnode<'a>(
    v: &'a XmlElement,
    indent: usize,
    bodies: &HashMap<&'a str, &'a str>,
    headlines: &mut HashMap<&'a str, &'a str>,
    out: &mut String,
) {
    let gnx = v.attributes.get("t").map(String::as_str);
    let ind = "  ".repeat(indent);

    match v.children.iter().find(|c| c.name == "vh") {
        Some(vh) => {
            let headline = vh.text_content.as_str();
            if let Some(gnx) = gnx {
                headlines.insert(gnx, headline);
            }
            out.push_str(&format!("{ind}* {headline}\n"));
            if let Some(body) = gnx.and_then(|gnx| bodies.get(gnx)) {
                render_body(out, body, indent);
            }
            for child in v.children.iter().filter(|c| c.name == "v") {
                render_vnode(child, indent + 1, bodies, headlines, out);
            }
        }
        // An empty stub: a clone reference to a node fully defined elsewhere
        // in the file. Show its headline (recorded from that first, full
        // occurrence) without repeating its body/children.
        None => match gnx.and_then(|gnx| headlines.get(gnx)) {
            Some(headline) => out.push_str(&format!("{ind}* {headline} (clone)\n")),
            None => out.push_str(&format!("{ind}* (clone)\n")),
        },
    }
}

/// A node's body as an indented `|`-piped block, one source line per line —
/// always a block, even for a single-line body, so a headline line never
/// carries body text that could be confused with the headline itself.
fn render_body(out: &mut String, body: &str, indent: usize) {
    if body.trim().is_empty() {
        return;
    }
    let block_indent = "  ".repeat(indent + 1);
    for line in body.trim_end().lines() {
        out.push_str(&block_indent);
        if line.trim().is_empty() {
            out.push('|');
        } else {
            out.push_str("| ");
            out.push_str(line.trim_end());
        }
        out.push('\n');
    }
}
