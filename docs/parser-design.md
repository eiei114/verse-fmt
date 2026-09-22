# Parser decision: pinned CST plus an independent lexical guard

Status: implemented for the bounded formatter, including guarded writes. UEFN acceptance
is pending. This is not an assertion of full Verse grammar coverage.

## Evidence

We compiled `taku25/tree-sitter-verse` revision
`6b5433e37b52c03c4c07f9468bf7cc11e45f2f82` with tree-sitter 0.25.10 on Rust
1.98.1, Windows MSVC. A self-authored 14-case probe covered a minimal device,
2-space indentation, nested comments, unterminated comments, nested string
interpolation, `<#>` comments, Unicode identifiers, paths, dot blocks, markup,
tabs, braced function bodies, typed top-level values and sibling methods.

Correction: the original probe was initially attributed to the repository's
1.97.0 pin, but a direct Rust binary earlier in PATH actually ran 1.98.1. The
verification script now explicitly resolves rustup's pinned compiler; the full
current regression suite and release build have since passed on 1.97.0.

The important observations were:

- A complete minimal creative device, 2-space indents, nested comments,
  interpolation with inner quoted strings, paths, dot blocks and braced
  bodies parse without ERROR nodes.
- An unterminated `<#` comment also parses without ERROR. This is deliberate
  upstream error recovery and must not count as successful analysis.
- A `<#>` indented comment can be misinterpreted as a short comment followed
  by code identifiers. Formatting it is unsafe without a correct comment parser.
- A Unicode identifier, markup expression and top-level `Count:int=1` were
  not accepted by this grammar. Rejection means unsupported by this tool,
  not necessarily invalid Verse.
- A concern about sibling methods escaping a class was disproved by
  inspecting node ancestry and ranges. The original grammar puts both
  methods inside the class. An experimental scanner change was discarded;
  **vendored source is unmodified**.

The installed Epic Verse VSIX version `0.0.58011042`, associated with UEFN
`++Fortnite+Release-42.20-CL-58011042-Windows`, was inspected for lexical
reference only. It has distinct string/interpolation, nested-comment and
indented-comment handling. Its code and grammar are not copied into this repo.
Official web pages for comments/code blocks/strings could not be extracted
through the static HTTP route; do not cite them as independently verified here.

## Choice and alternatives

Adopt the pinned MIT-licensed grammar as a concrete syntax tree (CST), while
retaining the entire original source and every lexical token. Generate edits,
not source from a lossy abstract syntax tree. Vendored C means no Node.js,
grammar generator, runtime download or third parser repository is needed.

A custom full Verse parser was not implemented or benchmarked. Its appeal is
control over whitespace, but its grammar maintenance cost is not justified
for this first slice. A line/regex-only formatter cannot adequately protect
interpolation, nested comments or block ownership and was rejected.

## Independent safeguards

1. Validate UTF-8, NUL, BOM and LF/CRLF, with explicit byte limits.
2. Scan all bytes into lossless tokens. Strings include their interpolated
   expressions as protected spans. Reject unclosed strings/comments/brackets,
   unsupported escapes, `<#>` comments and unsupported lexical forms.
3. Bound lexical nesting to 256, lexical tokens to 250,000, parse time to two
   seconds, CST depth to 512 and fingerprint events to 1,000,000.
4. Reject every ERROR or MISSING CST node. Unknown constructs fail with exit 2;
   they are not silently copied and reported as supported.
5. Reparse output and compare the full CST shape and all non-trivia lexical
   tokens, including exact literal/comment bytes. Tree comparison detects
   changing a method's containing block even with identical code tokens.
6. Render again and require byte-identical output. Refuse source output/write
   on a failed invariant rather than falling back to an unsafe formatter.

The layout now compares an independent normalized per-code-line indentation
signature before and after rendering. Structural indentation increases require
a recognized block introducer; tabs and ambiguous continuations fail closed.
Delimited continuations and protected multiline interiors retain indentation.
This is deliberately narrower than the complete language. CST comparison and
the indentation signature are not proofs of language semantics.
Compiler/build and runtime checks in UEFN remain separate required gates.

## Reproducible checks

`cargo test --locked` includes the relevant lexical and CST regressions.
`tests/fixtures/provenance.md` records authorship. Upstream identity/hashes and
license are in `vendor/tree-sitter-verse/upstream.md`. Local probe logs under
`target/research` are supplemental, not required to rebuild or run tests.
