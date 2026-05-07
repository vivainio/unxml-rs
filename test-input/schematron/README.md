# Peppol Schematron samples

Real-world schematron files used as reference inputs for `unxml --schematron`.
These are **not** part of the regression test suite (the test runner's glob is
non-recursive); the small fixture in `test-input/sample.sch` covers regressions.

## Source

OpenPEPPOL `peppol-bis-invoice-3`, downloaded 2026-05-07:

- `PEPPOL-EN16931-UBL.sch` — https://raw.githubusercontent.com/OpenPEPPOL/peppol-bis-invoice-3/master/rules/sch/PEPPOL-EN16931-UBL.sch
- `CEN-EN16931-UBL.sch` — https://raw.githubusercontent.com/OpenPEPPOL/peppol-bis-invoice-3/master/rules/sch/CEN-EN16931-UBL.sch

## Usage

```bash
./target/release/unxml --schematron test-input/schematron/PEPPOL-EN16931-UBL.sch
```

Pre-rendered outputs (`*.pug`) are committed alongside the `.sch` inputs so the
shape of the transformation is visible without running the tool. They are
regenerated manually when the upstream files change.
