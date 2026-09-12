//! `unxml outline <FILES...>`: print a flat landmark list (sections, method
//! definitions/calls, XSLT templates/calls) with source line spans, for
//! skimming one document or building a catalog across a whole set of them
//! without reading each in full. See `outline::render_outline` for the entry
//! recognizers and output format.

use anyhow::{Context, Result};
use clap::Parser;

use crate::outline::{Dialect, render_outline};
use crate::parse::{InputFormat, detect_format, expand_file_args, parse_xml, read_file_lenient};

#[derive(Parser)]
#[command(name = "unxml outline")]
#[command(about = "List sections, method calls, and XSLT templates with source line spans")]
struct OutlineArgs {
    /// XML/XSLT files to summarize (glob patterns supported, e.g. workflows/*.xml)
    files: Vec<String>,

    /// Recognize --special business-workflow landmarks (section, builtInMethodParameterList, method)
    #[arg(long)]
    special: bool,

    /// Recognize XSLT landmarks (xsl:template, xsl:function, xsl:call-template, xsl:apply-templates)
    #[arg(long)]
    xslt: bool,

    /// Autodetect --xslt from each file's extension (.xsl/.xslt)
    ///
    /// --special has no reliable extension signal (it's plain .xml), so it is
    /// never inferred; pass --special explicitly when scanning such files.
    #[arg(long)]
    auto: bool,
}

fn dialect_for(file_path: &str, args: &OutlineArgs) -> Dialect {
    if args.special {
        Dialect::Special
    } else if args.xslt {
        Dialect::Xslt
    } else if args.auto {
        let ext = std::path::Path::new(file_path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "xsl" || ext == "xslt" {
            Dialect::Xslt
        } else {
            Dialect::Generic
        }
    } else {
        Dialect::Generic
    }
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    let args = OutlineArgs::parse_from(
        std::iter::once("unxml outline".to_string()).chain(args.iter().cloned()),
    );

    if args.files.is_empty() {
        anyhow::bail!("No files specified. Provide at least one file or glob pattern.");
    }

    let files = expand_file_args(&args.files)?;
    if files.is_empty() {
        anyhow::bail!("No files found matching the specified patterns.");
    }

    let multiple = files.len() > 1;
    let mut combined = String::new();
    for (i, file_path) in files.iter().enumerate() {
        let content = match read_file_lenient(file_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading file '{file_path}': {e}");
                continue;
            }
        };

        if detect_format(&content, file_path) != InputFormat::Xml {
            eprintln!("Skipping {file_path}: `unxml outline` only supports XML/XSLT input");
            continue;
        }

        let parsed = parse_xml(&content).with_context(|| format!("failed to parse {file_path}"))?;
        let dialect = dialect_for(file_path, &args);
        let rendered = render_outline(&parsed.roots, dialect);

        if i > 0 {
            combined.push('\n');
        }
        if multiple {
            combined.push_str(&format!("// FILE: {file_path}\n"));
        }
        combined.push_str(&rendered);
    }

    print!("{combined}");
    Ok(())
}
