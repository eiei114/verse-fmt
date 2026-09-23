# Test source provenance

All `.verse` fixtures and inline snippets in this project's Rust tests were
authored for verse-fmt. They use public Verse syntax and API names; no Epic
sample project, assets, generated digest, or proprietary source was copied.
They use the project's MIT OR Apache-2.0 license.

`device.input.verse` is a minimal self-authored creative-device example.
`device.expected.verse` is its intended formatting, not compiler output.
UEFN compile and runtime acceptance have not yet been performed. Parser and
formatter test success must not be presented as UEFN acceptance.

`corpus.json` adds 87 named, categorized self-authored cases with independent
golden outputs or explicit rejection expectations. Categories cover encodings,
declarations, literals, expressions, collections, attributes/specifiers,
statements/control, block ownership/layout and protected spans. Rejection rows
include both malformed input and known valid-but-unsupported forms; these are
not interchangeable compiler claims. Map literals and top-level inline binary
function bodies were investigated after the first run failed; raw probe evidence
identified upstream grammar gaps. Dotted local imports, initialized typed
constants, comma-separated braced enums, empty class base lists, if-binding
conditions, failable indexed `set` conditions, key/value iterators, and tested
indented object/array construction with anonymous field initializers now use a
documented local grammar delta with independent positive regressions. Map literals
and the top-level binary-function ambiguity remain conservatively rejected.

`src/robustness.rs` uses fixed xorshift seeds (recorded in code), 256 structured
layout/property cases, 1,024 arbitrary-byte cases, deep/token-dense refusals and
a one-MiB protected literal. These do not replace an exhaustive fuzzer or UEFN.
