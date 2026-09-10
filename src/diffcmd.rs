//! `unxml diff <BASE> <MODIFIED>`: generate a patch sidecar that turns
//! `BASE` into `MODIFIED`. See `diff::generate` for the algorithm and its
//! deliberate limits.

use std::fs;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::diff::generate;
use crate::parse::{detect_format, parse_xml, read_file_lenient};
use crate::patch::render_all;

#[derive(Parser)]
#[command(name = "unxml diff")]
#[command(
    about = "Generate a patch sidecar file describing the difference between two XML documents"
)]
struct DiffArgs {
    /// The original document
    base: String,
    /// The changed document
    modified: String,
    /// Write the patch to this file instead of stdout
    #[arg(short, long)]
    out: Option<String>,
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    let args =
        DiffArgs::parse_from(std::iter::once("unxml diff".to_string()).chain(args.iter().cloned()));

    let base_content = read_file_lenient(&args.base)?;
    let modified_content = read_file_lenient(&args.modified)?;

    for (path, content) in [
        (&args.base, &base_content),
        (&args.modified, &modified_content),
    ] {
        if detect_format(content, path) != crate::parse::InputFormat::Xml {
            bail!("{path}: `unxml diff` only supports XML input, not HTML or JSON");
        }
    }

    let base =
        parse_xml(&base_content).with_context(|| format!("failed to parse {}", args.base))?;
    let modified = parse_xml(&modified_content)
        .with_context(|| format!("failed to parse {}", args.modified))?;

    let ops = generate(&base.roots, &modified.roots)?;
    let rendered = render_all(&ops);

    match args.out {
        Some(path) => {
            fs::write(&path, rendered).with_context(|| format!("failed to write {path}"))?
        }
        None => print!("{rendered}"),
    }
    Ok(())
}
