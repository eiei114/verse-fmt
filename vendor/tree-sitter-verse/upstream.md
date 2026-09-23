# Pinned upstream source

- Repository: https://github.com/taku25/tree-sitter-verse
- Revision: `6b5433e37b52c03c4c07f9468bf7cc11e45f2f82`
- License: MIT, retained verbatim in `LICENSE` (copyright 2025 taku25).
- Base revision files: `grammar.js`, `src/parser.c`, `src/scanner.c`,
  `src/tree_sitter/{parser,alloc,array}.h`. The grammar and generated parser
  are locally patched/regenerated. The scanner was initially byte-identical,
  then locally hardened to bound its serialized indentation stack.
- Base parser.c SHA-256: `48d6a8af6ecbb12260f0d563d6c7d81c8ce37e33984ebd7e56ab23ad58c56b2e`
- Base scanner.c SHA-256: `19cde62cd35695010453006f19601393a82747231ddfecd81a6a5eff70e59712`
- Local scanner.c SHA-256: `470901f9b9408fa552d7cbd14538d45362ef2fb492908643cbb1fdc465288006`

Generated C is vendored so builds need neither Node.js nor network access to
the upstream grammar. Cargo dependencies still need a cache or registry access.
No Epic proprietary grammar or compiler is redistributed. Updates require an
explicit revision/hash change and regression tests, not a build-time download.

Local reproducible delta from that source revision: allow local dotted names in
`using { ... }`, initialized typed constants at file scope and in executable
blocks, comma-separated braced enum variants, empty class base lists, if-binding
conditions, failable indexed `set` conditions, key/value iterators, and tested
indented object/array construction with anonymous field initializers. Generated
metadata targets ABI 15. The generated parser requires the upstream tree-sitter
0.25.10 runtime headers retained in `LICENSE-tree-sitter` under their separate
MIT notice. The local scanner hardening caps its indentation stack to keep
worst-case serialized state (one byte plus two `uint32_t` fields plus stack
entries) within Tree-sitter's 1024-byte buffer. Extreme indentation depth is
refused; comment/string tokenization is unchanged. `hashes.json` pins every grammar,
generated output, header, and license. Regeneration uses
`scripts/verify-grammar.ps1 -Regenerate`; compare its generated hashes with the
manifest. Normal Cargo builds remain offline from the grammar/tool and use the
committed generated C.

The pinned base parser is not a Verse compiler and its recovery behavior remains
insufficient for formatting safety by itself; see `../../docs/parser-design.md`.
