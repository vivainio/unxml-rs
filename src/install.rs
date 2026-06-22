//! `--install-skills`: copy the bundled Claude Code skills into the user's
//! `~/.claude/skills/` so the agent can discover them.
//!
//! The whole `skills/` tree is embedded at build time, so adding a new skill
//! directory — or dropping extra files (reference docs, examples) next to a
//! `SKILL.md` — is enough; it gets installed too, no code change needed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};

/// The skills tree, embedded at build time from `skills/`.
static SKILLS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skills");

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
