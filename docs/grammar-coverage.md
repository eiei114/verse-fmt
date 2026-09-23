# Syntax and formatting coverage

This is a coverage table, not a Verse language specification. All currently
successful rows are parser/formatter tests, **not UEFN compiler acceptance**.
The tool is an unreleased alpha and never replaces the UEFN compiler.

| Form | Current handling | Evidence |
|---|---|---|
| `using { /Fortnite.com/Devices }` | Parsed; path unchanged | Minimal device fixture |
| `Count := 1` | Parsed; assignment spacing/trailing space/final newline | stdin and idempotence tests |
| Class/field/method, effect/specifier annotations | Confirmed spaced blocks normalized to four-space levels | Device, 2-space-indent, block-ownership tests |
| `()` / `[]` / `{}` nesting | Lexically balanced; grammar must accept the expression | Delimiter rejection tests, parser probe |
| Nested `<# ... #>` comments | Protected byte-for-byte, strict closing delimiter required | Lexer roundtrip/negative tests |
| `#` line comments | Protected through last non-newline byte | Comment EOF and CRLF tests |
| String interpolation with nested quoted expressions | Entire outer string protected | CLI and lexer nested-string tests |
| Unicode in strings/comments | Protected, diagnostic scalar columns supported | Unicode position and stdin tests |
| Braced and dot control bodies | Parsed in probe; no style conversion | Probe; expand golden coverage before transformations |
| Tabs | Structural indentation tabs refused; protected text preserved | Indentation refusal tests; no guessed tab width |
| `<#>` indented comments | Unsupported, exit 2 | Guard regression; upstream misclassification |
| Markup expressions | Unsupported, exit 2 | Guard/CLI regression |
| Quoted or non-ASCII identifiers | Unsupported, exit 2 | Lexer/grammar coverage limit |
| Multiline string literals outside interpolation | Unsupported, exit 2 | Strict lexical policy |
| Initialized typed constants at file scope and in executable blocks | Locally supported; UEFN compile acceptance pending | CST scope assertion; golden/idempotence fixtures |
| Map literal `map{"x" => 1}` | Unsupported by pinned grammar, exit 2 | Expanded corpus and parser probe |
| Top-level inline binary function body `Add(X:int,Y:int):int = X+Y` | Reject same-line sibling ambiguity; grammar otherwise mis-splits body | Expanded corpus; explicit guard in both tools |
| String beginning with unescaped `#`, e.g. `A := "# text"` | Pinned scanner may misclassify; fails coverage checks, exit 2 | Linter rule investigation; never alter the literal to make it parse |
| Malformed/unclosed input | Rejected, exit 2 | Recovery, delimiter, quote tests |
| Mixed LF/CRLF | Preserved exactly in `preserve`; forced conversion follows protected-span policy | Source and CLI tests |
| Bare CR, UTF-16, invalid UTF-8, NUL | Rejected, exit 2 | Source and CLI tests |

Unknown syntax fails rather than returning an unchanged file as successful
formatting. All accepted forms preserve their CST and non-trivia token bytes.
No import sorting, bracket/call rewriting, comment reflow, identifier changes,
line wrapping, block-style conversion or type/effect analysis is implemented.
Whitespace adjacent to CST-confirmed calls/operators, type colons and separators
is normalized without changing their tokens. Blank-line runs outside protected
regions are reduced to one. Explicit EOL conversion is refused if it would
change bytes inside a multiline string/comment.

Before a public supported release, extend this table with fixture IDs, compiler
build results and real-project validation. A passing parser probe is only an
initial signal; it is not sufficient release evidence.

`tests/fixtures/corpus.json` maps 71 named inputs to independent golden output or
refusal. Categories include normal/boundary/rejection forms, rather than merely
counting whitespace permutations. Calls/index brackets are compacted only at
CST-confirmed openers; this does not distinguish failable calls from indexing
semantically. Fixed-seed tests additionally vary indent width, BOM, EOL and
protected Unicode, and feed bounded arbitrary bytes. A startup-inclusive local
Windows performance baseline is recorded in
[`performance-baseline-2026-09-23.md`](performance-baseline-2026-09-23.md); it is
informational, not a guarantee, and does not substitute for UEFN acceptance.

Real-project read-only inspection exposed additional practical coverage gaps:
dotted local imports (`using { Demo.Helpers }`), comma-separated braced enums,
and typed local constants such as `Label:string="hello"`. The local grammar now
supports these shapes; self-authored minimal positive regressions live in
`tests/coverage-gaps.rs`. Rejection of remaining forms means parser coverage debt,
not compiler diagnostics.
Parser recovery can also lose enclosing structure, so an "adjacent top-level"
error does not prove the original construct really was top-level. No private
project sources are included in this repository. Broad UEFN-project readiness
has not been demonstrated.
