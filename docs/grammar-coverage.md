# Syntax and formatting coverage

This is a coverage table, not a Verse language specification. All currently
supported rows are parser/formatter tests, **not UEFN compiler acceptance**.
The tool is an unreleased alpha and never replaces the UEFN compiler.

The table is machine-checked by `tests/docs-drift.rs`: each row states whether
the pinned build accepts the form or refuses it with exit 2, and names the
self-authored minimal fixture in `tests/fixtures/coverage.json` that proves it.
A row that disagrees with the binary fails the test with the form, the stated
handling and the observed exit code. Adding a row therefore means adding its
fixture; a fixture no row uses also fails.

The **UEFN-build column is unverified**. No row here records what the UEFN
compiler does with the form: that needs a human-run UEFN build and access to a
real project, which this repository does not have. Every row is a statement
about `verse-fmt` only. The column stays absent on purpose rather than being
filled with guesses or omitted silently; a later slice (upstream issue #42's
remaining work) adds it with recorded build results.

| Form | Handling | Evidence | Fixture |
|---|---|---|---|
| `using { /Fortnite.com/Devices }` | supported; path unchanged | Minimal device fixture | `using-path` |
| `Count := 1` | supported; assignment spacing/trailing space/final newline | stdin and idempotence tests | `assignment` |
| Class/field/method, effect/specifier annotations | supported; confirmed spaced blocks normalized to four-space levels | Device, 2-space-indent, block-ownership tests | `class-annotations` |
| `()` / `[]` / `{}` nesting | supported; lexically balanced, grammar must accept the expression | Delimiter rejection tests, parser probe | `delimiters` |
| Nested `<# ... #>` comments | supported; protected byte-for-byte, strict closing delimiter required | Lexer roundtrip/negative tests | `nested-block-comment` |
| `#` line comments | supported; protected through last non-newline byte | Comment EOF and CRLF tests | `line-comment` |
| String interpolation with nested quoted expressions | supported; entire outer string protected | CLI and lexer nested-string tests | `interpolation` |
| Unicode in strings/comments | supported; protected, diagnostic scalar columns supported | Unicode position and stdin tests | `unicode-literal` |
| Braced and dot control bodies | supported; parsed, no style conversion between them | Probe; expand golden coverage before transformations | `control-bodies`, `dot-control-body` |
| Tabs | refused (exit 2); structural indentation tabs refused, protected text preserved | Indentation refusal tests; no guessed tab width | `tab-indentation` |
| Indentation up to 252 scanner levels | supported (exit 0); the deepest indentation whose scanner state still serializes | Scanner boundary probe | `shallow-indentation` |
| Indentation at 253 scanner levels or deeper | refused (exit 2); 253 levels are the first refused depth, because the scanner's 253-entry stack must fit Tree-sitter's 1024-byte state buffer | Scanner boundary probe | `deep-indentation` |
| `<#>` indented comments | refused (exit 2); upstream misclassification | Guard regression | `indented-hash-comment` |
| Markup expressions | refused (exit 2); base grammar rejects the construct | Guard/CLI regression | `markup` |
| Quoted or non-ASCII identifiers | refused (exit 2); lexer/grammar coverage limit | Lexer/grammar coverage limit | `quoted-identifier`, `non-ascii-identifier` |
| Multiline string literals outside interpolation | refused (exit 2); strict lexical policy | Strict lexical policy | `multiline-string` |
| Initialized typed constants at file scope and in executable blocks | supported; UEFN compile acceptance pending | CST scope assertion; golden/idempotence fixtures | `typed-constants` |
| Empty class base list `class()` | supported | Self-authored practical-shape regression | `empty-base-list` |
| If-binding condition `if: Value := Expr then:` | supported | Self-authored formatter/linter regression | `if-binding` |
| Failable set assignment in an if condition | supported in tested indexed-target form | Self-authored parser ownership and golden regression | `failable-set` |
| Key/value iterator `for (Key -> Value : Values)` | supported in tested form | Self-authored iterator-field and golden regression | `key-value-iterator` |
| Indented object/array construction and anonymous field initializer `{Z := 5}` | supported in tested shape | Self-authored nested-construction regression | `indented-construction` |
| Map literal `map{"x" => 1}` | refused (exit 2); unsupported by pinned grammar | Expanded corpus and parser probe | `map-literal` |
| Top-level inline binary function body `Add(X:int,Y:int):int = X+Y` | refused (exit 2); same-line sibling ambiguity, grammar otherwise mis-splits body | Expanded corpus; explicit guard in both tools | `inline-binary-body` |
| Guard refusal that also contains a raw parse ERROR | refused (exit 2); guard stays primary, the parse error is a secondary failure with its offset and node kind | `tests/diagnostics-stage.rs`, self-authored fixtures | `guard-with-parse-error` |
| String beginning with unescaped `#`, e.g. `A := "# text"` | refused (exit 2); pinned scanner may misclassify | Linter rule investigation; never alter the literal to make it parse | `hash-prefixed-string` |
| Malformed/unclosed input | refused (exit 2) | Recovery, delimiter, quote tests | `unclosed-block-comment`, `unclosed-string` |
| Mixed LF/CRLF | supported; preserved exactly in `preserve`, forced conversion follows protected-span policy | Source and CLI tests | `mixed-line-endings` |
| Bare CR, UTF-16, invalid UTF-8, NUL | refused (exit 2); never silently replaced | Source and CLI tests | `bare-cr`, `utf16-source`, `invalid-utf8`, `nul-byte` |

Unknown syntax fails rather than returning an unchanged file as successful
formatting. All accepted forms preserve their CST and non-trivia token bytes.
No import sorting, bracket/call rewriting, comment reflow, identifier changes,
line wrapping, block-style conversion or type/effect analysis is implemented.
Whitespace adjacent to CST-confirmed calls/operators, type colons and separators
is normalized without changing their tokens. Blank-line runs outside protected
regions are reduced to one. Explicit EOL conversion is refused if it would
change bytes inside a multiline string/comment.

`tests/fixtures/corpus.json` maps 87 named inputs to independent golden output or
explicit refusal. Corpus counts: 87 cases total, 64 golden and 23 refusal.
That split is asserted by `tests/docs-drift.rs`, so no document can quote a
count the fixture file contradicts. Categories include normal/boundary/rejection
forms, rather than merely counting whitespace permutations. Calls/index brackets
are compacted only at CST-confirmed openers; this does not distinguish failable
calls from indexing semantically. Fixed-seed tests additionally vary indent
width, BOM, EOL and protected Unicode, and feed bounded arbitrary bytes. A
startup-inclusive local Windows performance baseline is recorded in
[`performance-baseline-2026-09-23.md`](performance-baseline-2026-09-23.md); it is
informational, not a guarantee, and does not substitute for UEFN acceptance.

Real-project read-only inspection exposed additional practical coverage gaps:
dotted local imports (`using { Demo.Helpers }`), comma-separated braced enums,
and typed local constants such as `Label:string="hello"`. The local grammar now
supports these shapes plus empty class base lists, if-binding conditions,
failable indexed `set` conditions, key/value iterators, and tested indented
object/array construction with anonymous field initializers; self-authored
minimal positive regressions live in `tests/coverage-gaps.rs`.
Rejection of remaining forms means parser coverage debt,
not compiler diagnostics.
Parser recovery can also lose enclosing structure, so an "adjacent top-level"
error does not prove the original construct really was top-level. A guard
refusal that also contains a raw parse ERROR reports both — the guard as the
primary reason and the parse error as a secondary one — so the guard message is
never mistaken for the only problem. See [parser design](parser-design.md).
No private project sources are included in this repository. Broad UEFN-project readiness
has not been demonstrated.

The bounded P1 extension accepts `for (I := 1..3)` range generators,
including tested bounds, comma-separated generators/filters and nested bodies.
It does not enable general intermediate `:=` bindings inside a header.
See [range-binding scope and evidence](range-binding.md). All unsupported-form
refusals remain tool coverage limits, not claims that Verse rejects those forms.
