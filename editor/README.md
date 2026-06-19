# Editor support for unxml output (`.unxml`)

`unxml` emits a Pug-like format. Name those files `*.unxml` (or force the
language) to get syntax highlighting.

- `unxml.sublime-syntax` — grammar for Sublime Text / **bat** (the `syntect`
  engine), installed by the script below.
- **VS Code / Cursor** — use the [JadeView extension](https://github.com/vivainio/jadeview),
  which bundles the same `.unxml` grammar (plus HTML→Pug and unxml rendering
  commands). The VS Code language extension is no longer published here.

## Quick install (bat)

```sh
python3 editor/install-editor-support.py
```

Or by hand:

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

## VS Code / Cursor

Install **JadeView** from <https://github.com/vivainio/jadeview> (download the
`.vsix` from the [latest release](https://github.com/vivainio/jadeview/releases/latest)
and run **Extensions: Install from VSIX…**).

With an XML/XSLT/XSD/Schematron file open, run the **Unxml: Render** command
from the Command Palette to render the current file as `.unxml` in a new tab
(JadeView shells out to the `unxml` binary on your `PATH`). The output is
highlighted automatically; for other files, run **Change Language Mode** and
pick *UnXML*.
