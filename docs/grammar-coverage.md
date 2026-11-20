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
| Braced and dot control bodies | supported; parsed, no style conversion between them | `tests/coverage-gaps.rs` golden/idempotence cases for both shapes | `control-bodies`, `dot-control-body` |
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
| Multi-line `if:` condition block (bindings, `set`, `not`, comparisons, one per line) | supported; one condition per line | `if-block-multi-condition` corpus case; CST item-ownership test | `if-block-multi-condition` |
| `case` arm with indented statement block or `{ ... }` body | supported; `=>` spacing preserved | `case-arm-block-bodies` corpus case; CST body-ownership test | `case-arm-block-bodies` |
| Indented `spawn:` with one expression | supported | `indented-spawn` corpus case; CST body-ownership test | `indented-spawn` |
| Nonempty map literal with `=>` entries | supported | `map-literal-pairs` corpus case; CST ownership test | `map-literal-pairs` |
| Empty indented archetype | supported; ambiguous next-line braces refused | `empty-indented-archetype` corpus case; CST ownership test | `empty-indented-archetype` |
| Whitespace before function effects | supported | `space-before-function-effects` corpus case; CST ownership test | `space-before-function-effects` |
| Indented enum with variant commas | supported | `indented-enum-commas` corpus case; CST ownership test | `indented-enum-commas` |
| Constructor declaration with an archetype body | supported | `constructor-declaration` corpus case; CST ownership test | `constructor-declaration` |
| Filter binding in a `for` header | supported | `for-filter-binding` corpus case; CST ownership test | `for-filter-binding` |
| Tuple type in a parametric archetype head | supported | `tuple-type-in-archetype-head` corpus case; CST ownership test | `tuple-type-in-archetype-head` |
| Failable set assignment in an if condition | supported in tested indexed-target form | Self-authored parser ownership and golden regression | `failable-set` |
| Key/value iterator `for (Key -> Value : Values)` | supported in tested form | Self-authored iterator-field and golden regression | `key-value-iterator` |
| Indented object/array construction and anonymous field initializer `{Z := 5}` | supported in tested shape | Self-authored nested-construction regression | `indented-construction` |
| Map literal `map{"x" => 1}` | supported | Expanded corpus and parser probe | `map-literal` |
| Top-level inline function body with operators or an archetype, e.g. `Add(X:int,Y:int):int = X+Y` | supported; the body owns the whole expression on its line | `inline-binary-body` and `inline-archetype-body` corpus cases; CST tests | `inline-binary-body` |
| Unseparated statements on one line, e.g. `First := 1 Second := 2` | refused (exit 2); newlines are not separators, so the grammar splits the line without ERROR | `reject-unseparated-statements` corpus case; guard in both tools | `unseparated-statements` |
| Call opener on the callee's line, including `Sum (1, 2)` | supported; spacing before a confirmed opener is removed, and a `(` on the next line starts a new statement | `spaced-call-opener` and `next-line-paren-statement` corpus cases; CST tests | `spaced-call-opener`, `next-line-paren-statement` |
| Qualified names in expressions, e.g. `(super:)Show()`, `(local:)Size`, `(/Example.org/Geometry:)point{}` | supported | `qualified-super-call`, `path-qualified-expression` and `qualified-definition-name` corpus cases | `qualified-super-call`, `path-qualified-expression` |
| `external {}` function body | supported | `external-function-body` corpus case | `external-function-body` |
| `;` between statements and inside `if (...)`, `for (...)` and a parenthesized sequence | supported; one space after `;`, none before | `semicolon-separators` and `semicolon-top-level` corpus cases | `semicolon-separators` |
| Typed definition with `:=`, e.g. `Limit:int := 10`, `Double(X:int):int :=` | supported | `typed-colon-equals` corpus case | `typed-colon-equals` |
| `defer:` and `defer{...}` blocks | supported | `defer-block` corpus case | `defer-block` |
| Named argument without `?`, e.g. `Make(Width := 1)` | supported | `named-argument` corpus case | `named-argument` |
| Attribute prefixing a definition on its own line, e.g. `@marker_scope marker := class(attribute):` | supported; only a declaration or another attribute may follow an attribute on one line | `prefix-attribute-same-line` corpus case; guard tests | `prefix-attribute-same-line` |
| Attribute before an enum variant, e.g. `@doc("first")` then `First` | supported | `enum-variant-attributes` corpus case | `enum-variant-attributes` |
| Attribute with an indented block of field initializers, e.g. `@editable:` then `Hint := true` | supported | `attribute-block-arguments` corpus case | `attribute-block-arguments` |
| Accessor specifiers, e.g. `var Count<getter(GetCount)><setter(SetCount)>:int` | supported | `accessor-specifiers` corpus case | `accessor-specifiers` |
| Empty base list on a struct or interface, e.g. `interface():`, `struct():`, `interface(){}` | supported | `empty-base-lists` corpus case | `empty-base-lists` |
| `block:` clause in a class body | supported | `class-block-clause` corpus case | `class-block-clause` |
| Indented `if:` condition block without `then` | supported | `if-condition-block-only` corpus case | `if-condition-block-only` |
| `for (...) do Expr` and a dot body holding `set` | supported | `for-do-and-dot-set` corpus case | `for-do-and-dot-set` |
| Parameter typed by a function signature, e.g. `Callback(:int):void` | supported | `function-typed-parameter` corpus case | `function-typed-parameter` |
| `<# ... #>` block comments after code, at file scope, trailing a statement or inside a block | supported; protected byte-for-byte | `block-comments-mid-file` corpus case; scanner regression tests | `block-comments-mid-file` |
| Two statements joined across a multi-line block comment, e.g. `A := 1 <# a` / `#> B := 2` | refused (exit 2); the comment does not separate them, at file scope or inside a block | `reject-comment-joined-statements` and `reject-comment-joined-top-level` corpus cases | `comment-joined-statements` |
| Statement line starting with unary `+`/`-` after another expression, e.g. `X := A` then `-2 < A` | refused (exit 2); newlines are not separators, so the grammar joins it as a binary operand | `reject-leading-unary-continuation` corpus case; guard regression in both tools | `reject-leading-unary-continuation` |
| Guard refusal that also contains a raw parse ERROR | refused (exit 2); guard stays primary, the parse error is a secondary failure with its offset and node kind | `tests/diagnostics-stage.rs`, self-authored fixtures | `guard-with-parse-error` |
| String beginning with unescaped `#`, e.g. `A := "# text"` | refused (exit 2); pinned scanner may misclassify | Linter rule investigation; never alter the literal to make it parse | `hash-prefixed-string` |
| `for: ... do:` | supported; ambiguous clause boundaries refused | `indented-for-do` corpus case and CST tests | `indented-for-do` |
| Typed `var` initializer with `:=` | supported; mutable declaration CST retained | `var-colon-equals` corpus case and CST tests | `var-colon-equals` |
| Braced comma-separated and indented `option` expressions | supported; ambiguous postfix continuations refused | `option-block-and-comma-list` corpus case and CST tests | `option-block-and-comma-list` |
| Array and `logic` type arguments in archetype heads | supported | `archetype-array-and-logic-arguments` corpus case and CST tests | `archetype-array-and-logic-arguments` |
| Mixed parenthesized and indented `else if` clauses | supported | `mixed-else-if-chain` corpus case and CST tests | `mixed-else-if-chain` |
| Empty indented case arm before an outer sibling | supported; enclosing dedents are retained | `empty-case-arm-before-dedent` corpus case and CST tests | `empty-case-arm-before-dedent` |
| Shallower final empty `_ =>` in a case | supported when the line still belongs to the case and the following content exits it; aligned to sibling arms where normalizing block indentation | Golden, refusal and CST ownership tests | `shallower-empty-case-wildcard` |
| Inline `set` after a case arrow | supported; the assignment belongs to its arm | `case-inline-set` corpus case and CST tests | `case-inline-set` |
| Braced expression list in a `:=` declaration | supported with two or more comma-delimited expressions | `braced-expression-list` corpus case and CST tests | `braced-expression-list` |
| Indexed `set` in an indented `for` header | supported | `for-header-set-clause` corpus case and CST tests | `for-header-set-clause` |
| Adjacent call in the first indented `for` iterator | supported, with line-leading call ambiguity still refused | `for-header-iterable-call` corpus case and CST tests | `for-header-iterable-call` |
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

`tests/fixtures/corpus.json` maps the named inputs to independent golden output or
explicit refusal. Corpus counts: 141 cases total, 115 golden and 26 refusal.
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
Later comma-delimited clauses also accept filter bindings.
See [range-binding scope and evidence](range-binding.md). All unsupported-form
refusals remain tool coverage limits, not claims that Verse rejects those forms.
