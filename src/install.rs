//! `--install-skills`: copy the bundled Claude Code skills into the user's
//! `~/.claude/skills/` so the agent can discover them.
//!
//! The whole `skills/` tree is embedded at build time, so adding a new skill
//! directory — or dropping extra files (reference docs, examples) next to a
//! `SKILL.md` — is enough; it gets installed too, no code change needed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};

/// The skills tree, embedded at build time from `skills/`.
static SKILLS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skills");

/// The `bat`/`batcat` Sublime grammar for `.unxml` output, embedded at build
/// time so `--install-bat` works from a plain `cargo install` with no
/// checkout on disk (unlike `editor/install-editor-support.py`, which this
/// supersedes for the bat half of that script).
pub(crate) static BAT_SYNTAX: &str = include_str!("../editor/unxml.sublime-syntax");
const BAT_SYNTAX_NAME: &str = "unxml.sublime-syntax";

/// Resolve the user's home directory across platforms (`HOME`, then
/// `USERPROFILE` on Windows).
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("Could not determine home directory (neither HOME nor USERPROFILE is set)")
}

/// Recursively write an embedded directory's contents under `dest`, creating
/// directories as needed and overwriting existing files.
fn extract_dir(dir: &Dir<'_>, dest: &Path) -> Result<usize> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create directory {}", dest.display()))?;

    let mut count = 0;
    for file in dir.files() {
        // Embedded paths are relative to the include root; take the basename.
        let name = file.path().file_name().expect("embedded file has a name");
        let out = dest.join(name);
        std::fs::write(&out, file.contents())
            .with_context(|| format!("Failed to write {}", out.display()))?;
        count += 1;
    }
    for sub in dir.dirs() {
        let name = sub.path().file_name().expect("embedded dir has a name");
        count += extract_dir(sub, &dest.join(name))?;
    }
    Ok(count)
}

/// Install the bundled skills to `~/.claude/skills/`, overwriting any existing
/// copies.
pub(crate) fn install_skills() -> Result<()> {
    let dir = home_dir()?.join(".claude").join("skills");
    let count = extract_dir(&SKILLS_DIR, &dir)?;
    println!("Installed {count} skill file(s) to {}", dir.display());
    Ok(())
}

// --- `--install-bat`: register the .unxml grammar with bat -----------------

/// Copy the embedded Sublime grammar into `bat`'s config dir and rebuild its
/// syntax cache, so `bat file.unxml` (and `unxml --bat`) get highlighting.
/// Looks for `bat`, then `batcat` (the Debian/Ubuntu package name). Errors if
/// neither is on PATH.
pub(crate) fn install_bat() -> Result<()> {
    let bat = which("bat")
        .or_else(|| which("batcat"))
        .context("`bat` (or `batcat`) not found on PATH — install it first")?;

    let out = Command::new(&bat)
        .arg("--config-dir")
        .output()
        .with_context(|| format!("Failed to run `{} --config-dir`", bat.display()))?;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "`{} --config-dir` failed: {}",
            bat.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let config_dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let dest_dir = Path::new(&config_dir).join("syntaxes");
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create directory {}", dest_dir.display()))?;
    let dest = dest_dir.join(BAT_SYNTAX_NAME);
    std::fs::write(&dest, BAT_SYNTAX)
        .with_context(|| format!("Failed to write {}", dest.display()))?;
    println!("Installed grammar to {}", dest.display());

    let status = Command::new(&bat)
        .args(["cache", "--build"])
        .status()
        .with_context(|| format!("Failed to run `{} cache --build`", bat.display()))?;
    if !status.success() {
        return Err(anyhow::anyhow!("`{} cache --build` failed", bat.display()));
    }
    println!("Rebuilt bat cache — try: bat file.unxml, or unxml --bat file.xml");
    Ok(())
}

/// Minimal `which`: search `PATH` for an executable named `name` (trying
/// `name` and `name.exe`, for Windows). Avoids pulling in the `which` crate
/// for this one lookup.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        [dir.join(name), dir.join(format!("{name}.exe"))]
            .into_iter()
            .find(|candidate| candidate.is_file())
    })
}

// --- `unxml git`: transient textconv wrapper --------------------------------
//
// Wraps `git <args>` with the unxml diff driver applied for that one
// invocation only, via `-c` config overrides and a throwaway attributes
// file — nothing is written under `.git/`, so it never affects any other
// tool or command that shells out to git in the repo (plain `git diff`,
// an IDE's diff view, `tig`, ...).

/// File globs bound to the unxml diff driver. `--auto` then picks the right
/// dialect mode (xslt/xsd/schematron/…) from each extension.
const GIT_PATTERNS: &[&str] = &[
    "*.xml", "*.xsl", "*.xslt", "*.xsd", "*.wsdl", "*.sch", "*.html", "*.htm", "*.json", "*.leo",
];

/// The textconv command passed via `-c`. Assumes `unxml` is on PATH (the
/// normal `cargo install --path .` outcome).
const GIT_TEXTCONV: &str = "unxml --canonical --auto";

/// Run `git <args>` with the unxml textconv driver applied for this
/// invocation only, then exit with git's exit code. Nothing is persisted:
/// the diff driver is passed via `-c`, and the file-pattern bindings live in
/// a temporary attributes file pointed to by a transient
/// `core.attributesFile` override.
pub(crate) fn git_passthrough(args: &[String]) -> Result<()> {
    let attrs_path =
        std::env::temp_dir().join(format!("unxml-git-attributes-{}", std::process::id()));
    let mut attrs = String::new();
    for pattern in GIT_PATTERNS {
        attrs.push_str(&format!("{pattern} diff=unxml\n"));
    }
    std::fs::write(&attrs_path, attrs)
        .with_context(|| format!("Failed to write {}", attrs_path.display()))?;

    let status = Command::new("git")
        .arg("-c")
        .arg(format!("diff.unxml.textconv={GIT_TEXTCONV}"))
        .arg("-c")
        .arg("diff.unxml.cachetextconv=true")
        .arg("-c")
        .arg(format!("core.attributesFile={}", attrs_path.display()))
        .args(args)
        .status()
        .context("Failed to run `git` (is it installed and on PATH?)");

    let _ = std::fs::remove_file(&attrs_path);

    let status = status?;
    std::process::exit(status.code().unwrap_or(1));
}
