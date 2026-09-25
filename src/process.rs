//! Orchestration: read input, parse, hide namespaces, select subtrees,
//! render, and emit (optionally through `bat`).

use std::collections::HashSet;
use std::io::{self, Read};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::canonical::canonicalize;
use crate::document::{
    HIDE_NS_ALL, hide_namespaces, is_cii_document, is_msbuild_document, is_ubl_document,
    sniff_hidden_prefixes,
};
use crate::inputs::ENTRY_SEP;
use crate::json::render_json;
use crate::model::{Collapse, FormatOpts, XmlElement};
use crate::parse::{InputFormat, decode_with_fallback, detect_format, parse_html, parse_xml};
use crate::paths::dump_paths;
use crate::pathsel::ordinal_among;
use crate::render::render_comment;
use crate::xpathmini::{Hit, XPathMini};
use crate::xslt::TemplateRegistry;

/// The cross-cutting, CLI-derived options shared by every input. Built once and
/// passed by reference, so the process functions stay narrow even as flags grow.
/// The per-file processing *mode* (`FormatOpts`) is passed separately because it
/// can vary per file under `--auto`.
pub(crate) struct ProcessOptions<'a> {
    pub(crate) format_override: Option<&'a str>,
    pub(crate) hide_ns: &'a HashSet<String>,
    pub(crate) sniff: bool,
    pub(crate) select: Option<&'a XPathMini>,
    pub(crate) canonical: bool,
    pub(crate) paths: bool,
    pub(crate) depth: usize,
    pub(crate) no_attrs: bool,
    pub(crate) fold: bool,
    pub(crate) expand: bool,
    pub(crate) output: OutputMode,
}

/// What each input's output is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum OutputMode {
    /// The rendered unxml text (the default).
    Text,
    /// `--jsonl`: one JSON object per hit, one per line.
    Jsonl,
    /// `--files-with-matches`: just the input's name, when it has a hit.
    FilesWithMatches,
}

/// Render one input. `from_latin1` says `content` was decoded with the
/// Latin-1 fallback, so `--jsonl` byte ranges must be mapped back to the
/// original bytes.
pub(crate) fn process_content(
    content: &str,
    file_path: &str,
    from_latin1: bool,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
    cfg: &ProcessOptions,
) -> Result<String> {
    // Determine input format
    let format = if let Some(format_str) = cfg.format_override {
        match format_str.to_lowercase().as_str() {
            "html" => InputFormat::Html,
            "xml" => InputFormat::Xml,
            "json" => InputFormat::Json,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported format: {}. Use 'xml', 'html', or 'json'",
                    format_str
                ));
            }
        }
    } else {
        detect_format(content, file_path)
    };

    if format == InputFormat::Json {
        if cfg.select.is_some() || cfg.paths || cfg.output != OutputMode::Text {
            return Err(anyhow::anyhow!(
                "--select, --paths, --jsonl and --files-with-matches are not yet supported for JSON"
            ));
        }
        return render_json(content, cfg.canonical, cfg.sniff);
    }

    // Searching many files: an XML document whose raw text lacks a name the
    // selector needs cannot match, so skip parsing it. (HTML is excluded —
    // its tag names are case-insensitive in the source.)
    if format == InputFormat::Xml
        && let Some(selector) = cfg.select
        && !selector
            .required_literals()
            .all(|lit| content.contains(lit))
    {
        return Ok(String::new());
    }

    // Parse the content based on detected/specified format. `top_comments` are
    // the prolog/epilog comments outside the root element (XML only); HTML has
    // no such concept here.
    let (mut elements, top_comments) = match format {
        InputFormat::Html => (
            parse_html(content, &format).context("Failed to parse HTML")?,
            Vec::new(),
        ),
        InputFormat::Xml => {
            let parsed = parse_xml(content).context("Failed to parse XML")?;
            (parsed.roots, parsed.top_comments)
        }
        InputFormat::Json => unreachable!("JSON returns before XML/HTML parsing"),
    };

    // Build the effective set of prefixes to hide: those requested explicitly,
    // plus any inferred by sniffing the document type (only under --auto/--bat).
    // The `ALL` sentinel hides every prefix regardless of the rest of the set.
    let mut hidden = cfg.hide_ns.clone();
    if cfg.sniff {
        hidden.extend(sniff_hidden_prefixes(&elements));
    }
    let hide_all = hidden.contains(HIDE_NS_ALL);
    if hide_all || !hidden.is_empty() {
        for element in &mut elements {
            hide_namespaces(element, &hidden, hide_all);
        }
    }

    // Build the effective mode/collapse opts before canonicalising, so a
    // content-sniffed mode also governs the sibling-sort decision below.
    let mut effective = opts.clone();

    // Under --auto/--bat, an MSBuild project/import file (`<Project>` root)
    // gets --msbuild even when its extension didn't already select it (e.g.
    // stdin, or an unrecognised extension) — unless the user already forced
    // an explicit mode.
    if cfg.sniff && !opts.has_mode() && elements.iter().any(is_msbuild_document) {
        effective.msbuild = true;
    }

    // Under --auto/--bat, a genuine UBL or CII instance folds its single-child
    // wrapper chains automatically (the same documents whose prefixes we hide),
    // unless the user already chose a --collapse mode. These vocabularies bury
    // content under deep scaffolding — UBL's `ext:UBLExtensions`, CII's nested
    // `ram:`/`rsm:` wrappers — and folding it never drops information (the tag
    // names stay on the path), while genuine multi-child aggregates are left
    // expanded.
    if cfg.sniff
        && matches!(opts.collapse, Collapse::Off)
        && elements
            .iter()
            .any(|e| is_ubl_document(e) || is_cii_document(e))
    {
        effective.collapse = Collapse::All;
    }
    let opts = &effective;

    // Canonicalise for diff-friendly output: always rebind prefixes to stable
    // names, but only sort siblings in plain XML mode — in a dialect/`--special`
    // mode element order is significant, so sorting would misrepresent it.
    if cfg.canonical {
        canonicalize(&mut elements, !opts.has_mode());
    }

    // Determine the roots to emit: the whole document, or just the subtrees
    // matched by --select.
    let hits: Vec<Hit> = if let Some(selector) = cfg.select {
        let matched = selector.select(&elements);
        // No match renders nothing at all (not even an empty --paths dump),
        // so callers can drop the file from multi-file output.
        if matched.is_empty() {
            return Ok(String::new());
        }
        matched
    } else {
        elements
            .iter()
            .map(|elem| Hit {
                elem,
                path: format!("{}[{}]", elem.name, ordinal_among(&elements, elem)),
            })
            .collect()
    };
    match cfg.output {
        OutputMode::Text => {}
        OutputMode::FilesWithMatches => return Ok(format!("{file_path}\n")),
        OutputMode::Jsonl => {
            return Ok(jsonl_records(
                &hits,
                content,
                file_path,
                from_latin1,
                opts,
                registry,
            ));
        }
    }
    let roots: Vec<&XmlElement> = hits.iter().map(|hit| hit.elem).collect();

    // --paths dumps the distinct element paths; otherwise render the tree. Under
    // --select, render each matched subtree as a fragment separated by a blank
    // line; the whole-document case emits roots back-to-back.
    let output = if cfg.paths {
        dump_paths(&roots, cfg.depth, cfg.no_attrs, cfg.fold)
    } else if cfg.select.is_some() {
        // --select renders matched subtrees as fragments; the document prolog
        // (top-level comments) is not part of any selected subtree, so omit it.
        let mut out = String::new();
        for (i, elem) in roots.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&elem.format_yaml_like(0, opts, registry));
        }
        out
    } else {
        // Whole document: interleave top-level (prolog/epilog) comments with the
        // roots at their recorded insertion points so a licence header or
        // trailing note renders where it stood. `top_comments` is empty for HTML
        // and for comment-free XML, so this matches the old output exactly.
        let mut out = String::new();
        for (i, elem) in roots.iter().enumerate() {
            for (idx, text) in &top_comments {
                if *idx == i {
                    render_comment(&mut out, text, 0);
                }
            }
            out.push_str(&elem.format_yaml_like(0, opts, registry));
        }
        for (idx, text) in &top_comments {
            if *idx == roots.len() {
                render_comment(&mut out, text, 0);
            }
        }
        out
    };

    Ok(output)
}

/// `--jsonl`: one JSON object per hit, so a search over many files is easy to
/// consume incrementally. `byte_range` is a half-open range into the input's
/// original bytes (for an archive entry, the entry's uncompressed bytes), so
/// the raw segment can be read back exactly; `xml` is that segment and
/// `text` its rendered form. Positions are absent for HTML, which the parser
/// doesn't track.
fn jsonl_records(
    hits: &[Hit],
    content: &str,
    file_path: &str,
    from_latin1: bool,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
) -> String {
    // A Latin-1 input was decoded one char per original byte, so its original
    // offset is the char count up to the decoded offset.
    let original_offset = |offset: usize| {
        if from_latin1 {
            content[..offset].chars().count()
        } else {
            offset
        }
    };
    let mut out = String::new();
    for hit in hits {
        let elem = hit.elem;
        let mut record = Map::new();
        record.insert("file".into(), file_path.into());
        if let Some((archive, entry)) = file_path.split_once(ENTRY_SEP) {
            record.insert("archive".into(), archive.into());
            record.insert("entry".into(), entry.into());
        }
        record.insert("path".into(), hit.path.clone().into());
        record.insert("name".into(), elem.name.clone().into());
        let mut attrs: Vec<_> = elem.attributes.iter().collect();
        attrs.sort();
        let attrs: Map<String, Value> = attrs
            .into_iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();
        record.insert("attrs".into(), attrs.into());
        if elem.start_line > 0 {
            record.insert("line_range".into(), json!([elem.start_line, elem.end_line]));
        }
        let source = elem
            .byte_range
            .and_then(|(start, end)| Some((start, end, content.get(start..end)?)));
        if let Some((start, end, _)) = source {
            record.insert(
                "byte_range".into(),
                json!([original_offset(start), original_offset(end)]),
            );
        }
        record.insert(
            "text".into(),
            elem.format_yaml_like(0, opts, registry).into(),
        );
        if let Some((_, _, xml)) = source {
            record.insert("xml".into(), xml.into());
        }
        out.push_str(&Value::Object(record).to_string());
        out.push('\n');
    }
    out
}

pub(crate) fn process_stdin(opts: &FormatOpts, cfg: &ProcessOptions) -> Result<String> {
    // Read from stdin, tolerating non-UTF-8 input (see read_file_lenient).
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .context("Failed to read from stdin")?;
    let (content, from_latin1) = decode_with_fallback(bytes);

    // Note: expand mode not supported for stdin since we need file paths for imports
    process_content(&content, "stdin", from_latin1, opts, None, cfg)
}

/// Emit rendered output, optionally through `bat` for syntax highlighting.
/// When `use_bat` is set we pipe to `bat -l unxml`; if no `bat` binary is
/// found we fall back to plain stdout so `--bat` degrades gracefully.
pub(crate) fn emit(output: &str, use_bat: bool) {
    if use_bat && pipe_to_bat(output) {
        return;
    }
    print!("{output}");
}

/// Try to display `output` via `bat -l unxml`. Returns true if a `bat` (or
/// `batcat`, the Debian/Ubuntu name) process was launched and handed the
/// output, false if no such binary exists.
pub(crate) fn pipe_to_bat(output: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    for bin in ["bat", "batcat"] {
        // Only stdin is piped; bat inherits our stdout/stderr so its pager
        // draws straight to the terminal.
        let mut child = match Command::new(bin)
            .args(["-l", "unxml"])
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue, // binary not found — try the next name
        };
        if let Some(mut stdin) = child.stdin.take() {
            // Ignore a broken pipe if the user quits the pager early.
            let _ = stdin.write_all(output.as_bytes());
        }
        let _ = child.wait();
        return true;
    }
    false
}
