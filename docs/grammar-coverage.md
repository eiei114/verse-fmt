# Syntax and formatting coverage

This is a coverage table, not a Verse language specification. All currently
successful rows are parser/formatter tests, **not UEFN compiler acceptance**.
The tool is an unreleased alpha and never replaces the UEFN compiler.

| Form | Current handling | Evidence |
|---|---|---|
| `using { /Fortnite.com/Devices }` | Parsed; path unchanged | Minimal device fixture |
| `Count := 1` | Parsed; assignment spacing/trailing space/final newline | stdin and idempotence tests |
| Class/field/method, effect/specifier annotations | Parsed; indentation unchanged in this slice | Device, 2-space-indent, block-ownership tests |
| `()` / `[]` / `{}` nesting | Lexically balanced; grammar must accept the expression | Delimiter rejection tests, parser probe |
| Nested `<# ... #>` comments | Protected byte-for-byte, strict closing delimiter required | Lexer roundtrip/negative tests |
| `#` line comments | Protected through last non-newline byte | Comment EOF and CRLF tests |
| String interpolation with nested quoted expressions | Entire outer string protected | CLI and lexer nested-string tests |
| Unicode in strings/comments | Protected, diagnostic scalar columns supported | Unicode position and stdin tests |
| Braced and dot control bodies | Parsed in probe; no style conversion | Probe; expand golden coverage before transformations |
| Tabs | Lexically accepted; original indentation retained in this slice | Probe; no guessed tab-to-space conversion |
| `<#>` indented comments | Unsupported, exit 2 | Guard regression; upstream misclassification |
| Markup expressions | Unsupported, exit 2 | Guard/CLI regression |
| Quoted or non-ASCII identifiers | Unsupported, exit 2 | Lexer/grammar coverage limit |
| Multiline string literals outside interpolation | Unsupported, exit 2 | Strict lexical policy |
| Top-level typed `Count:int=1` | Unsupported by pinned grammar, exit 2 | Grammar probe/regression |
| Malformed/unclosed input | Rejected, exit 2 | Recovery, delimiter, quote tests |
| Mixed LF/CRLF, UTF-16, invalid UTF-8, NUL | Rejected, exit 2 | Source and CLI tests |

Unknown syntax fails rather than returning an unchanged file as successful
formatting. All accepted forms preserve their CST and non-trivia token bytes.
No import sorting, bracket/call rewriting, comment reflow, identifier changes,
line wrapping, block-style conversion or type/effect analysis is implemented.

Before a public supported release, extend this table with fixture IDs, compiler
build results and real-project validation. A passing parser probe is only an
initial signal; it is not sufficient release evidence.
