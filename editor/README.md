# Editor support for unxml output (`.unxml`)

`unxml` emits a Pug-like format. Name those files `*.unxml` (or force the
language) to get syntax highlighting.

Two grammars are kept here — they describe the same language for two engines:

- `unxml.sublime-syntax` — Sublime Text / **bat** (the `syntect` engine).
- `vscode/` — a **VS Code** language extension wrapping a TextMate grammar
  (`vscode/syntaxes/unxml.tmLanguage.json`).

## Quick install (recommended)

Installs everything that applies on this machine — bat plus every VS Code /
Cursor directory found, including the Windows side when run from WSL. Works on
WSL, native Windows, Linux and macOS.

```sh
python3 editor/install-editor-support.py
```

Then **fully quit and restart VS Code** (a "Reload Window" may not pick up a
freshly copied extension). The manual steps below are the equivalent if you'd
rather do it by hand.

## bat

```sh
cp editor/unxml.sublime-syntax "$(bat --config-dir)/syntaxes/"
bat cache --build
```

Then:

```sh
unxml some.xsl > some.unxml
bat some.unxml            # auto-detected by extension
unxml some.xsl | bat -l unxml   # force the language for a pipe
unxml --bat some.xsl     # shortcut: unxml pipes through `bat -l unxml` itself
```

## VS Code

Copy the `vscode/` folder into your VS Code extensions directory and reload the
window (`Developer: Reload Window`).

- **Editing files inside WSL (Remote-WSL):**
  ```sh
  cp -r editor/vscode ~/.vscode-server/extensions/unxml-language-1.0.0
  ```
- **Editing files as Windows paths:**
  ```sh
  cp -r editor/vscode /mnt/c/Users/<you>/.vscode/extensions/unxml-language-1.0.0
  ```

Files ending in `.unxml` are then highlighted automatically. For other files,
run **Change Language Mode** and pick *UnXML*.

To package a redistributable `.vsix` instead, `cd vscode && npx @vscode/vsce package`.
