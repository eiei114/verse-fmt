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
- Local scanner.c SHA-256: `e176d6c7d07f251f3bc286985c6759da85852550f1b1da376ecd88b8faf3f035`

Generated C is vendored so builds need neither Node.js nor network access to
the upstream grammar. Cargo dependencies still need a cache or registry access.
No Epic proprietary grammar or compiler is redistributed. Updates require an
explicit revision/hash change and regression tests, not a build-time download.

Local reproducible delta from that source revision: allow local dotted names in
`using { ... }`, initialized typed constants at file scope and in executable
blocks, comma-separated braced enum variants, empty class base lists, if-binding
conditions (one item per line in the indented `if:` form), failable indexed
`set` conditions, key/value iterators, block-bodied `case` arms
(indented statements or `{ ... }`), single-expression indented `spawn:`, and tested
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

P1 additionally accepts `identifier := range_expression` in for_iterator only.
The scanner is unchanged.
ABI15 artifacts were regenerated with Tree-sitter CLI 0.25.10; hashes.json pins
the changed grammar and generated outputs. See `../../docs/range-binding.md`.

Call arguments admit a tuple type, including in a parametric archetype
head such as `event(tuple(int, logic)){}`. The archetype keeps its existing
call-shaped head and braced body.

Parenthesized `for` headers accept a later comma-delimited filter binding.
A direct range after `:=` retains its generator node. Filter bindings are
visible after their initializer in the loop's scope.

Declarations with `<constructor>` can use parameters, effects and a
`:=` archetype body. Ordinary parameterized `:=` definitions retain their
existing CST ownership.

Indented `enum:` bodies use the same variant list as braced enums, with
optional commas between variants and after the final variant.

Function signatures allow whitespace between their closing parameter
parenthesis and first effect specifier. Comparisons retain their operator
nodes and the remaining specifier openers retain their immediate syntax.

An indented archetype may omit all field initializers. Its scanner
indent/dedent pair still encloses the archetype and keeps following statements
at their original scope. The syntax adapter refuses a next-line brace
that the parser attaches to an empty archetype as another construction.

Nonempty map literals reuse the `map_literal` and `map_entry` rules
from verse-lint's `94acbac`. An entry-less `map{}` retains its existing
archetype node.

Indented `for: … do:` headers begin with an iterator. Later clauses reuse the
parenthesized iterator and filter-binding nodes. A guard rejects same-line
clauses without a comma and line-leading postfix openers that would otherwise
be absorbed by the preceding expression. The same check covers a postfix on\na new line after the loop result. Loop bindings stay in the loop scope.

A typed `var` declaration accepts `=` or `:=` before its initializer.
Both spellings keep the same mutable binding, type and initializer fields.

An `option` expression supports comma-separated values in braces and one
expression in an indented body. The guard refuses a line-leading postfix
that would attach to a previous expression across these newly accepted boundaries.

Call arguments admit unambiguous array types (`[]T`) and the bare `logic`
type keyword. `logic{...}` still parses as a logic expression, and the
archetype keeps its call-shaped head and braced body.

An indented `if: … then:` and parenthesized `if (...):` can share an
`else if` chain. Mixed alternatives preserve the condition/body ownership
and the following sibling. The linter marks mixed-clause semantic coverage
incomplete.

An empty indented block can end at the same newline as its enclosing blocks.
The scanner queues the enclosing dedents along with the phantom block's
dedent, keeping the first following statement in its outer scope.

A case arm may use an inline `set` assignment as its body. The assignment
retains the same CST fields as a `set` inside an indented arm.

A `:=` declaration can own a braced list of two or more comma-separated
expressions. Initializer braces with `Name := Value` keep the existing
struct-literal or construction nodes. A line-leading bare list following a
call stays unsupported because of the possible attachment to that call.

An indented `for` header can contain a `set` clause after its iterator.
The header retains the assignment and its operator.

When an indented for header starts with `Item : Read(Values)`, the adjacent
opener keeps the call inside the first iterator. The clause boundary guard
continues to reject a postfix attached across a newline.

A final empty wildcard can be shallower than the first arm if it is
still indented past the line that opens the case and the next content
line closes the case (or the file ends). This exception is limited to plain
space indentation and a bare wildcard arrow at this boundary. The
scanner keeps the case frame open while closing any preceding arm body.
The source check verifies the boundary using the line with the colon,
including when the header spans multiple lines.

A qualified name `(scope:)name` is an expression, so `(super:)Show()`,
`(local:)Size` and `(/Example.org/Geometry:)point{}` parse as a call, a
reference and a construction. A qualified generic type keeps the qualified name
as its `generic_type` name instead of nesting the type under the qualifier.

The scanner emits the external `_call_open` token for a `(` reached without
crossing a newline. Calls, declaration parameter lists, generic type arguments
and for-header iterable calls use it, so a `(` on the next line starts a new
item, as in the official grammar, while `Sum (1, 2)` stays a call. With that
boundary in place an inline function body no longer needs a precedence that
ends it before operators: it owns the whole expression on its line
(`Add(X:int,Y:int):int = X+Y`, `= point{...}`, `= external {}`).

`;` separates statements at file scope and in blocks, the items of an
`if (...)` condition and a `for (...)` header, and a parenthesized sequence
`(A; B)`. A typed constant, a field and a function or extension body may use
`:=` as well as `=`. A named argument may omit `?`. `defer` takes a colon,
brace or dot block like the other control expressions.

An attribute keeps a braced argument list in preference to a braced literal
that could start the next item, and it may take an indented block of field
initializers (`@editable:` followed by `Hint := true`). Attributes may prefix an
enum variant. Specifiers accept an accessor argument (`<getter(GetCount)>`,
`<setter(SetCount)>`). A struct or interface may have an empty base list, a class
body may hold a `block:` clause, and an indented `if:` condition block may stand
without `then`. A parenthesized `for` header may take `do` with an inline
expression or a block, a dot body may hold `set`, and a parameter may be typed
by a function signature (`Callback(:int):void`).

The scanner reads a `<#` comment wherever the grammar's lexer would otherwise
see `<`: after horizontal space, on a later line at file scope, and on the next
content line of a block when the indentation does not change. Only the comment
bytes form the token. Before this, a block comment anywhere but at the very
start of a scan was split into `<`, a line comment and stray tokens.

A type body and an indented field-initializer block accept `;` after an item.
An indented archetype (`point:` followed by `X := 1`) carries a dynamic
preference over a typed definition with `:=`, so a single-field archetype is
not read as `point:X := 1` across the line break.
