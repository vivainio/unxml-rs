//! Rendering many inputs — plain files and zip archive entries — in
//! parallel, while emitting the results in input order. Work proceeds in
//! bounded batches so memory stays flat however many files (or archive
//! entries) there are, and output streams out as each batch completes.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};

use anyhow::{Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use zip::ZipArchive;

use crate::document::detect_mode_from_ext;
use crate::model::{Collapse, FormatOpts};
use crate::parse::{InputFormat, decode_lenient, expand_file_args, format_from_extension};
use crate::process::{ProcessOptions, process_content};
use crate::xslt::TemplateRegistry;

/// Inputs rendered concurrently before their output is emitted in order.
const BATCH: usize = 512;

/// One rendered input: its display name (`archive.zip!/inner/path.xml` for
/// an archive entry) and the rendered text or the error it hit.
pub(crate) struct Rendered {
    pub(crate) name: String,
    pub(crate) result: Result<String>,
}

/// The per-run settings needed to render any single input.
pub(crate) struct Renderer<'a> {
    pub(crate) opts: &'a FormatOpts,
    /// Pick each input's mode from its extension (`--auto` without a forced mode).
    pub(crate) autodetect: bool,
    pub(crate) collapse: &'a Collapse,
    pub(crate) cfg: &'a ProcessOptions<'a>,
}

/// A zip archive to read, optionally narrowed to the entries matching a glob:
/// `bundle.zip` (every markup entry), `bundle.zip!/orders/42.xml` (one entry,
/// the same name a `// FILE:` header shows), or `bundle.zip!/orders/*.xml`.
pub(crate) struct ZipSpec {
    pub(crate) archive: String,
    entries: Option<glob::Pattern>,
}

/// Separates an archive path from the entry path inside it.
const ENTRY_SEP: &str = "!/";

impl ZipSpec {
    /// Whether a file argument names entries inside an archive.
    pub(crate) fn is_entry_arg(arg: &str) -> bool {
        arg.contains(ENTRY_SEP)
    }

    /// Parse `--zip` values (and `archive!/entry` file arguments). The archive
    /// part may itself be a glob, e.g. `dumps/*.zip!/orders/*.xml`.
    pub(crate) fn parse_all(args: &[String]) -> Result<Vec<Self>> {
        let mut specs = Vec::new();
        for arg in args {
            let (archive, entries) = match arg.split_once(ENTRY_SEP) {
                Some((archive, inner)) => {
                    let pattern = glob::Pattern::new(inner)
                        .with_context(|| format!("Invalid entry pattern in '{arg}'"))?;
                    (archive, Some(pattern))
                }
                None => (arg.as_str(), None),
            };
            for archive in expand_file_args(&[archive.to_string()])? {
                specs.push(Self {
                    archive,
                    entries: entries.clone(),
                });
            }
        }
        Ok(specs)
    }

    /// True if this names exactly one entry (no glob metacharacters), so it
    /// reads like a single file.
    pub(crate) fn is_single_entry(&self) -> bool {
        self.entries
            .as_ref()
            .is_some_and(|p| !p.as_str().contains(['*', '?', '[']))
    }

    /// Visit the selected entries in archive order, as `(display name,
    /// contents)`. Entries are decompressed one at a time, and only once
    /// their name is selected. A whole archive yields only entries that look
    /// like markup, so images, class files and the like are skipped silently;
    /// an explicit entry pattern yields whatever it names.
    pub(crate) fn for_each_entry(&self, mut visit: impl FnMut(String, Vec<u8>)) -> Result<()> {
        let path = &self.archive;
        let file = File::open(path).with_context(|| format!("Failed to open {path}"))?;
        let mut archive = ZipArchive::new(BufReader::new(file)).context("Not a valid zip file")?;
        let match_opts = glob::MatchOptions {
            require_literal_separator: true,
            ..Default::default()
        };
        let mut matched = false;
        for i in 0..archive.len() {
            let mut entry = match archive.by_index(i) {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("Error reading entry {i} of '{path}': {e}");
                    continue;
                }
            };
            if entry.is_dir()
                || self
                    .entries
                    .as_ref()
                    .is_some_and(|p| !p.matches_with(entry.name(), match_opts))
            {
                continue;
            }
            let name = format!("{path}{ENTRY_SEP}{}", entry.name());
            let mut bytes = Vec::new();
            if let Err(e) = entry.read_to_end(&mut bytes) {
                eprintln!("Error reading '{name}': {e}");
                continue;
            }
            if self.entries.is_none() && !looks_like_markup(&bytes) {
                continue;
            }
            matched = true;
            visit(name, bytes);
        }
        // A missing exact entry is an error; a glob may legitimately match
        // nothing in some of the archives it is applied to.
        if !matched && self.is_single_entry() {
            let name = self.entries.as_ref().map(glob::Pattern::as_str);
            anyhow::bail!("no entry named '{}'", name.unwrap_or_default());
        }
        Ok(())
    }
}

impl Renderer<'_> {
    /// Render `files`, then the selected entries of each archive in `zips`,
    /// handing each result to `emit` in input order.
    pub(crate) fn run(&self, files: &[String], zips: &[ZipSpec], mut emit: impl FnMut(Rendered)) {
        for chunk in files.chunks(BATCH) {
            let rendered: Vec<Rendered> = chunk
                .par_iter()
                .map(|path| Rendered {
                    name: path.clone(),
                    result: self.render_file(path),
                })
                .collect();
            rendered.into_iter().for_each(&mut emit);
        }
        for zip in zips {
            if let Err(e) = self.run_zip(zip, &mut emit) {
                eprintln!("Error reading zip '{}': {e:#}", zip.archive);
            }
        }
    }

    /// Stream an archive's entries through the renderer: decompressed
    /// sequentially (one archive handle), rendered in parallel batches.
    fn run_zip(&self, zip: &ZipSpec, emit: &mut impl FnMut(Rendered)) -> Result<()> {
        let mut batch: Vec<(String, Vec<u8>)> = Vec::new();
        let result = zip.for_each_entry(|name, bytes| {
            batch.push((name, bytes));
            if batch.len() >= BATCH {
                self.flush_entries(&mut batch, emit);
            }
        });
        self.flush_entries(&mut batch, emit);
        result
    }

    fn flush_entries(&self, batch: &mut Vec<(String, Vec<u8>)>, emit: &mut impl FnMut(Rendered)) {
        let rendered: Vec<Rendered> = batch
            .par_drain(..)
            .map(|(name, bytes)| {
                let result = self.render_bytes(&name, bytes, None);
                Rendered { name, result }
            })
            .collect();
        rendered.into_iter().for_each(emit);
    }

    fn render_file(&self, path: &str) -> Result<String> {
        let bytes = fs::read(path).with_context(|| format!("Failed to read file: {path}"))?;
        // --expand resolves xsl:import/include relative to the file on disk.
        let registry = if self.cfg.expand && self.file_opts(path).xslt {
            Some(TemplateRegistry::build_from_file(path)?)
        } else {
            None
        };
        self.render_bytes(path, bytes, registry.as_ref())
    }

    fn render_bytes(
        &self,
        name: &str,
        bytes: Vec<u8>,
        registry: Option<&TemplateRegistry>,
    ) -> Result<String> {
        if self.cannot_match(&bytes, name) {
            return Ok(String::new());
        }
        let content = decode_lenient(bytes);
        process_content(&content, name, &self.file_opts(name), registry, self.cfg)
    }

    fn file_opts(&self, name: &str) -> FormatOpts {
        let mut opts = if self.autodetect {
            detect_mode_from_ext(name)
        } else {
            self.opts.clone()
        };
        opts.collapse = self.collapse.clone();
        opts
    }

    /// Byte-level `--select` prefilter, run before decoding or format
    /// sniffing: a document known to be XML that lacks one of the selector's
    /// required literals can't match. `process_content` repeats the check on
    /// decoded text for inputs whose format is only known after sniffing;
    /// this earlier pass saves the decode for the common `.xml` case.
    /// Non-ASCII literals are left to that later check, since their bytes
    /// depend on the file's encoding.
    fn cannot_match(&self, bytes: &[u8], name: &str) -> bool {
        let Some(selector) = self.cfg.select else {
            return false;
        };
        let is_xml = match self.cfg.format_override {
            Some(format) => format.eq_ignore_ascii_case("xml"),
            None => format_from_extension(name) == Some(InputFormat::Xml),
        };
        is_xml
            && selector
                .required_literals()
                .filter(|lit| lit.is_ascii())
                .any(|lit| memmem::find(bytes, lit.as_bytes()).is_none())
    }
}

/// Whether an archive entry looks like XML/HTML: its first non-whitespace
/// byte (after any UTF-8 BOM) is `<`.
fn looks_like_markup(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

/// Where rendered output goes: straight to stdout as it is produced, or into
/// a buffer when the whole output must be post-processed (`--html`, `--cat`,
/// `--bat`).
pub(crate) enum Output {
    Buffer(String),
    Stdout(io::BufWriter<io::StdoutLock<'static>>),
}

impl Output {
    pub(crate) fn stdout() -> Self {
        Self::Stdout(io::BufWriter::new(io::stdout().lock()))
    }

    pub(crate) fn push(&mut self, text: &str) {
        match self {
            Self::Buffer(buf) => buf.push_str(text),
            Self::Stdout(out) => {
                if let Err(e) = out.write_all(text.as_bytes()) {
                    exit_on_write_error(e);
                }
            }
        }
    }

    /// Flush streamed output; returns the buffered text, if buffering.
    pub(crate) fn finish(self) -> Option<String> {
        match self {
            Self::Buffer(buf) => Some(buf),
            Self::Stdout(mut out) => {
                if let Err(e) = out.flush() {
                    exit_on_write_error(e);
                }
                None
            }
        }
    }
}

/// A closed pipe (`unxml ... | head`) just means the reader has had enough:
/// stop quietly rather than panicking like `print!` would.
fn exit_on_write_error(e: io::Error) -> ! {
    if e.kind() == io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    eprintln!("Error writing output: {e}");
    std::process::exit(1);
}
