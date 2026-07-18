# demo

The [unxml demos site](https://vivainio.github.io/unxml-demos/demos/) shows
`unxml` rendering real-world XML Schemas and XSLT stylesheets as
syntax-highlighted, full-page HTML. It is a [Zensical](https://zensical.org/)
site that lives entirely in the sibling
[`unxml-demos`](https://github.com/vivainio/unxml-demos) repo — generation,
vendored sources, and the CI workflow all live there now
(`scripts/generate-demos.py`), not here.

The site's CI installs `unxml` from PyPI (`pip install unxml`) and renders
against sources vendored under `examples/` in that repo — no network fetch,
no dependency on a locally built `unxml-rs` binary.

## Previewing changes ahead of a release

To preview how an unfinished `unxml-rs` change (e.g. a new flag) affects the
demo site before it's released to PyPI:

```sh
cargo build --release   # in this repo
cd ../unxml-demos
python3 scripts/generate-demos.py --unxml-bin ../unxml-rs/target/release/unxml
```

Then preview locally with `zensical serve` (or just check the diff) — no need
to commit unless you're happy with it. In CI, the pinned PyPI version in
`.github/workflows/docs.yml` is what actually gets deployed; bump it there
after a new `unxml-rs` release if the demo pages should reflect it.
