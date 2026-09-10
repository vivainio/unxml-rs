//! Patch ops and their application to a parsed document, in the spirit of
//! RFC 5261 (XML Patch Operations Using XPath) but narrower: five ops, each
//! anchored by a `sel` (see `pathsel.rs`) that must resolve to exactly one
//! node. `diff::generate` produces `Op` lists; `patch` (the CLI command)
//! loads them from a sidecar file and applies them here.
//!
//! `load_all`/`render_all` are *not* a general YAML parser/writer — they
//! only read and write the one fixed shape below (double-quoted scalars, a
//! `|` literal block for `content`), which is all a patch file authored by
//! `diff` or by hand following this shape ever needs. A real YAML dependency
//! would buy generality this format deliberately doesn't want.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};

use crate::model::{NodeRef, XmlElement};
use crate::parse::parse_xml;
use crate::pathsel::resolve_parent_mut;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    /// Inserts `content` (one or more well-formed XML fragments) as the next
    /// sibling(s) of `sel`.
    InsertAfter { sel: String, content: String },
    /// Inserts `content` as the previous sibling(s) of `sel` — the
    /// first-child-insertion counterpart to `InsertAfter`.
    InsertBefore { sel: String, content: String },
    /// Removes the node matched by `sel`.
    Remove { sel: String },
    /// Replaces the text content of the (leaf) element matched by `sel`.
    ReplaceText { sel: String, text: String },
    /// Sets attribute `name` to `value` on the element matched by `sel`.
    SetAttr {
        sel: String,
        name: String,
        value: String,
    },
}

impl Op {
    pub(crate) fn sel(&self) -> &str {
        match self {
            Op::InsertAfter { sel, .. }
            | Op::InsertBefore { sel, .. }
            | Op::Remove { sel }
            | Op::ReplaceText { sel, .. }
            | Op::SetAttr { sel, .. } => sel,
        }
    }
}

/// Render `ops` to the sidecar form `load_all` reads back.
pub(crate) fn render_all(ops: &[Op]) -> String {
    let mut out = String::new();
    for op in ops {
        match op {
            Op::SetAttr { sel, name, value } => {
                out.push_str("- op: set-attr\n");
                out.push_str(&format!("  sel: {}\n", quote(sel)));
                out.push_str(&format!("  name: {}\n", quote(name)));
                out.push_str(&format!("  value: {}\n", quote(value)));
            }
            Op::ReplaceText { sel, text } => {
                out.push_str("- op: replace-text\n");
                out.push_str(&format!("  sel: {}\n", quote(sel)));
                out.push_str(&format!("  text: {}\n", quote(text)));
            }
            Op::Remove { sel } => {
                out.push_str("- op: remove\n");
                out.push_str(&format!("  sel: {}\n", quote(sel)));
            }
            Op::InsertAfter { sel, content } => {
                out.push_str("- op: insert-after\n");
                out.push_str(&format!("  sel: {}\n", quote(sel)));
                out.push_str("  content: |\n");
                append_block(&mut out, content);
            }
            Op::InsertBefore { sel, content } => {
                out.push_str("- op: insert-before\n");
                out.push_str(&format!("  sel: {}\n", quote(sel)));
                out.push_str("  content: |\n");
                append_block(&mut out, content);
            }
        }
    }
    out
}

fn append_block(out: &mut String, text: &str) {
    for line in text.split('\n') {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn unquote(s: &str) -> Result<String> {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        bail!("expected a double-quoted value, got: {s}");
    }
    let inner = &s[1..s.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Ok(out)
}

/// Parse a patch sidecar file's contents: a list of single-purpose op
/// mappings, in the exact shape `render_all` produces.
pub(crate) fn load_all(text: &str) -> Result<Vec<Op>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut ops = Vec::new();
    let mut n = 0;
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        let Some(kind) = lines[i].strip_prefix("- op:") else {
            bail!("expected '- op: <kind>' at line {}: {:?}", i + 1, lines[i]);
        };
        let kind = kind.trim().to_string();
        n += 1;
        i += 1;
        let mut fields: HashMap<String, String> = HashMap::new();
        while i < lines.len() && !lines[i].starts_with("- op:") {
            let raw = lines[i];
            let trimmed = raw.trim_start();
            if trimmed.is_empty() {
                i += 1;
                continue;
            }
            let Some((key, rest)) = trimmed.split_once(':') else {
                bail!("op #{n}: bad field line {}: {:?}", i + 1, raw);
            };
            let key = key.trim().to_string();
            let rest = rest.trim();
            if rest == "|" {
                i += 1;
                let mut block = Vec::new();
                while i < lines.len()
                    && !lines[i].starts_with("- op:")
                    && (lines[i].starts_with("    ") || lines[i].is_empty())
                {
                    block.push(
                        lines[i]
                            .strip_prefix("    ")
                            .unwrap_or(lines[i])
                            .to_string(),
                    );
                    i += 1;
                }
                if block.last().is_some_and(|l| l.is_empty()) {
                    block.pop();
                }
                fields.insert(key, block.join("\n"));
            } else {
                fields.insert(
                    key.clone(),
                    unquote(rest).with_context(|| format!("op #{n}, field '{key}'"))?,
                );
                i += 1;
            }
        }
        ops.push(build_op(&kind, fields, n)?);
    }
    Ok(ops)
}

fn build_op(kind: &str, mut f: HashMap<String, String>, n: usize) -> Result<Op> {
    let mut req = |key: &str| -> Result<String> {
        f.remove(key)
            .ok_or_else(|| anyhow::anyhow!("op #{n} ({kind}): missing '{key}'"))
    };
    Ok(match kind {
        "set-attr" => Op::SetAttr {
            sel: req("sel")?,
            name: req("name")?,
            value: req("value")?,
        },
        "replace-text" => Op::ReplaceText {
            sel: req("sel")?,
            text: req("text")?,
        },
        "remove" => Op::Remove { sel: req("sel")? },
        "insert-after" => Op::InsertAfter {
            sel: req("sel")?,
            content: req("content")?,
        },
        "insert-before" => Op::InsertBefore {
            sel: req("sel")?,
            content: req("content")?,
        },
        other => bail!(
            "op #{n}: unknown op '{other}' (known: insert-after, insert-before, remove, replace-text, set-attr)"
        ),
    })
}

/// Parse `content` as one or more well-formed XML fragments, in order, ready
/// to splice into a parent's children. Wrapped in a synthetic root purely to
/// give the parser a single well-formed document — unlike a DOM-based
/// approach, no namespace scaffolding is needed here, since unxml is
/// namespace-oblivious throughout (a prefix is just literal name text).
fn parse_fragment(content: &str) -> Result<Vec<XmlElement>> {
    let wrapped = format!("<unxml-patch-fragment>{content}</unxml-patch-fragment>");
    let parsed = parse_xml(&wrapped).context("patch content is not well-formed XML")?;
    Ok(parsed
        .roots
        .into_iter()
        .next()
        .map(|r| r.children)
        .unwrap_or_default())
}

/// Apply `ops` in order to `base`, mutating it in place. Callers that
/// generated `ops` themselves (`diff::generate`'s self-check) can rely on
/// this being the same code path a hand-authored patch file goes through.
pub(crate) fn apply(base: &mut Vec<XmlElement>, ops: &[Op]) -> Result<()> {
    for op in ops {
        apply_one(base, op).with_context(|| format!("applying op with sel '{}'", op.sel()))?;
    }
    Ok(())
}

fn apply_one(base: &mut Vec<XmlElement>, op: &Op) -> Result<()> {
    match op {
        Op::SetAttr { sel, name, value } => {
            let (mut parent, idx) = resolve_parent_mut(base, sel)?;
            parent.children_mut()[idx]
                .attributes
                .insert(name.clone(), value.clone());
        }
        Op::ReplaceText { sel, text } => {
            let (mut parent, idx) = resolve_parent_mut(base, sel)?;
            let target = &mut parent.children_mut()[idx];
            target.text_content = text.clone();
            target.children.clear();
            target.nodes = vec![NodeRef::Text(text.clone())];
        }
        Op::Remove { sel } => {
            let (mut parent, idx) = resolve_parent_mut(base, sel)?;
            parent.remove_child(idx);
        }
        Op::InsertAfter { sel, content } => {
            let fragments = parse_fragment(content)?;
            let (mut parent, idx) = resolve_parent_mut(base, sel)?;
            parent.insert_children(idx + 1, fragments);
        }
        Op::InsertBefore { sel, content } => {
            let fragments = parse_fragment(content)?;
            let (mut parent, idx) = resolve_parent_mut(base, sel)?;
            parent.insert_children(idx, fragments);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(xml: &str) -> Vec<XmlElement> {
        parse_xml(xml).unwrap().roots
    }

    #[test]
    fn set_attr_adds_or_changes() {
        let mut doc = parse_one("<Invoice id=\"1\"><Line/></Invoice>");
        apply(
            &mut doc,
            &[Op::SetAttr {
                sel: "Invoice[1]".to_string(),
                name: "id".to_string(),
                value: "2".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc[0].attributes.get("id").unwrap(), "2");
    }

    #[test]
    fn replace_text_on_a_leaf() {
        let mut doc = parse_one("<Invoice><Note>old</Note></Invoice>");
        apply(
            &mut doc,
            &[Op::ReplaceText {
                sel: "Invoice[1]/Note[1]".to_string(),
                text: "new".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc[0].children[0].text_content, "new");
    }

    #[test]
    fn remove_a_child() {
        let mut doc = parse_one("<Invoice><Line/><Line/><Line/></Invoice>");
        apply(
            &mut doc,
            &[Op::Remove {
                sel: "Invoice[1]/Line[2]".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc[0].children.len(), 2);
    }

    #[test]
    fn insert_after_preserves_order_for_multiple_fragments() {
        let mut doc = parse_one("<Invoice><Line id=\"1\"/><Line id=\"3\"/></Invoice>");
        apply(
            &mut doc,
            &[Op::InsertAfter {
                sel: "Invoice[1]/Line[1]".to_string(),
                content: "<Line id=\"2a\"/><Line id=\"2b\"/>".to_string(),
            }],
        )
        .unwrap();
        let ids: Vec<&str> = doc[0]
            .children
            .iter()
            .map(|c| c.attributes["id"].as_str())
            .collect();
        assert_eq!(ids, ["1", "2a", "2b", "3"]);
    }

    #[test]
    fn insert_before_first_child() {
        let mut doc = parse_one("<Invoice><Line id=\"2\"/></Invoice>");
        apply(
            &mut doc,
            &[Op::InsertBefore {
                sel: "Invoice[1]/Line[1]".to_string(),
                content: "<Line id=\"1\"/>".to_string(),
            }],
        )
        .unwrap();
        let ids: Vec<&str> = doc[0]
            .children
            .iter()
            .map(|c| c.attributes["id"].as_str())
            .collect();
        assert_eq!(ids, ["1", "2"]);
    }

    #[test]
    fn insert_preserves_a_sibling_comment() {
        let mut doc = parse_one("<Invoice><!-- keep me --><Line id=\"1\"/></Invoice>");
        apply(
            &mut doc,
            &[Op::InsertAfter {
                sel: "Invoice[1]/Line[1]".to_string(),
                content: "<Line id=\"2\"/>".to_string(),
            }],
        )
        .unwrap();
        let has_comment = doc[0]
            .nodes
            .iter()
            .any(|n| matches!(n, NodeRef::Comment { text, .. } if text.trim() == "keep me"));
        assert!(has_comment, "{:?}", doc[0].nodes);
        assert_eq!(doc[0].children.len(), 2);
    }

    #[test]
    fn ambiguous_sel_errors() {
        let mut doc = parse_one("<Invoice><Line/><Line/></Invoice>");
        let err = apply(
            &mut doc,
            &[Op::Remove {
                sel: "Invoice[1]/Line".to_string(),
            }],
        )
        .unwrap_err();
        // `{err}` only prints the outermost `.with_context` message; the
        // underlying "ambiguous" cause is further down the chain.
        assert!(format!("{err:#}").contains("ambiguous"), "{err:#}");
    }

    #[test]
    fn render_and_load_round_trip() {
        let ops = vec![
            Op::SetAttr {
                sel: "Invoice[1]/ID[1]".to_string(),
                name: "schemeID".to_string(),
                value: "0088".to_string(),
            },
            Op::ReplaceText {
                sel: "Invoice[1]/Note[1]".to_string(),
                text: "a \"quoted\" note\nwith a newline".to_string(),
            },
            Op::Remove {
                sel: "Invoice[1]/Line[3]".to_string(),
            },
            Op::InsertAfter {
                sel: "Invoice[1]/Line[2]".to_string(),
                content: "<Line id=\"x\"/>".to_string(),
            },
            Op::InsertBefore {
                sel: "Invoice[1]/Line[1]".to_string(),
                content: "<Line id=\"y\"/>".to_string(),
            },
        ];
        let text = render_all(&ops);
        let loaded = load_all(&text).unwrap();
        assert_eq!(loaded, ops);
    }
}
