//! `unxml patch <BASE> <PATCH>`: apply a patch sidecar file (as produced by
//! `unxml diff`, or hand-written to the same shape) to `BASE` and print the
//! resulting XML.
//!
//! v1 limitation: only the root element(s) are re-serialized — a document
//! prolog/epilog comment outside the root (`parse::ParsedXml::top_comments`)
//! is dropped rather than round-tripped, since `diff`/`patch` don't anchor
//! ops on those. Worth fixing if a real patch needs to touch one.

use std::fs;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::parse::{detect_format, parse_xml, read_file_lenient};
use crate::patch::{apply, load_all};
use crate::xmlwrite::write_elements;

#[derive(Parser)]
#[command(name = "unxml patch")]
#[command(about = "Apply a patch sidecar file to a base XML document")]
struct PatchArgs {
    /// The document to patch
    base: String,
    /// The patch sidecar file (as produced by `unxml diff`)
    patch: String,
    /// Write the result to this file instead of stdout
    #[arg(short, long)]
    out: Option<String>,
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    let args = PatchArgs::parse_from(
        std::iter::once("unxml patch".to_string()).chain(args.iter().cloned()),
    );

    let base_content = read_file_lenient(&args.base)?;
    if detect_format(&base_content, &args.base) != crate::parse::InputFormat::Xml {
        bail!(
            "{}: `unxml patch` only supports XML input, not HTML or JSON",
            args.base
        );
    }
    let patch_text = read_file_lenient(&args.patch)?;

    let mut base = parse_xml(&base_content)
        .with_context(|| format!("failed to parse {}", args.base))?
        .roots;
    let ops = load_all(&patch_text).with_context(|| format!("failed to parse {}", args.patch))?;

    apply(&mut base, &ops)?;

    let rendered = write_elements(&base);
    match args.out {
        Some(path) => {
            fs::write(&path, rendered).with_context(|| format!("failed to write {path}"))?
        }
        None => print!("{rendered}"),
    }
    Ok(())
}
