# Bounded range-generator support (P1)

The parser accepts `for (I := 1..3)` using a dedicated alternative whose
iterable must be a `range_expression`. This is not a blanket addition of `:=`
to all iterator separators. Intermediate bindings such as `Twice := I * 2`
inside a header remain unsupported, as do non-range `I := Values` headers.
These refusals describe tool coverage, not compiler invalidity.

## Syntax evidence

Epic's [Book of Verse reference](https://dev.epicgames.com/documentation/en-us/fortnite/verse-language-book-of-verse-reference)
links to the [Book of Verse](https://verselang.github.io/book/07_control/#range-iteration).
The range-iteration section documents `:=` with inclusive range bounds;
the defining-variables section separately describes intermediate bindings.
Epic warns that this reference follows development main and can include
unreleased features. It is not acceptance evidence for the installed UEFN.
Observed Book repository main: `2af9dfd79c46ce91623e00c9088a342842ad785b`;
the evidence was the live rendered documentation, not a pinned source capture.

## Safety and coverage

- Hand-authored structural tests assert variable/iterable and bound ownership,
  unary/additive/multiplicative grouping, ordered generators/filters, nested
  loop owners and post-loop/sibling-function separation.
- CLI goldens cover compact input, parentheses, nested bodies and BOM/CRLF with
  protected Unicode. Malformed bounds and unimplemented bindings still refuse.
- Scanner, lexical/CST/token/indentation checks and write adapter are unchanged.
  The existing formatting pass handles `:=`; no new whitespace heuristic.
- Grammar C/JSON regenerated with Tree-sitter CLI 0.25.10, ABI15. Independent
  copies and hashes are maintained in each tool; no runtime coupling added.
- Original fixtures contain no private project code. Passing tests do not
  establish type/effect correctness, UEFN compilation or full project coverage.
