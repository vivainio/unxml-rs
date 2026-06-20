# demo

The [unxml demos site](https://vivainio.github.io/unxml-demos/demos/) shows
`unxml` rendering real-world XML Schemas and XSLT stylesheets as
syntax-highlighted, full-page HTML. It is a [Zensical](https://zensical.org/)
site that lives in the sibling [`unxml-demos`](https://github.com/vivainio/unxml-demos)
repo (clone it next to this one as `../unxml-demos`).

Nothing is vendored here: `publish-to-demo-site.py` holds a manifest of
canonical source URLs, downloads each into a gitignored cache (`demo/.cache/`),
renders it with the local `unxml` binary, highlights it with `bat` (using the
`unxml` grammar) piped to `ansi2html`, and writes one self-contained full-page
HTML document per demo into the site repo. Pages are grouped by category
(`schemas/`, `xslt/`) and share one created-once stylesheet (`demos/ansi.css`).

## Publishing

Prerequisites:

```sh
cargo build --release                       # the unxml binary the script runs
python3 editor/install-editor-support.py    # install the unxml grammar into bat
pip install ansi2html
```

Then, from the repo root (needs network on first run / after clearing the cache):

```sh
python3 demo/publish-to-demo-site.py            # writes to ../unxml-demos
python3 demo/publish-to-demo-site.py PATH/TO/unxml-demos   # or an explicit path
```

Commit and push `unxml-demos` afterwards — its GitHub Actions workflow rebuilds
and deploys the site.

### Adding a demo

Add a row to the `DEMOS` manifest in `publish-to-demo-site.py`:

```python
("xslt", "docbook/inline", "DocBook XSL — inline elements", f"{DOCBOOK}/html/inline.xsl"),
#  mode   output slug      index title                       source URL
```

`mode` (`xsd` / `xslt`) selects both the `unxml --<mode>` flag and the category
subdir/section. The slug becomes the page path under `demos/<category>/`.

### Regenerating the stylesheet

`demos/ansi.css` is created once and reused, because the colour palette is fixed
by the bat theme × the `unxml` grammar, not by content. Each run warns if a page
references a colour the committed css lacks (e.g. after adding a new document
type, changing the bat theme, or upgrading bat). When that happens:

```sh
python3 demo/publish-to-demo-site.py --regen-css
```
