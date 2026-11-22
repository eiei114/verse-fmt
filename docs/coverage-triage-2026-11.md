# Coverage family triage, 2026-11

This is a planning document. It changes no grammar, scanner, guard or test, and it
creates no issue. It exists so that a maintainer can approve one next step instead of
reading nine equally weighted requests.

Evidence rules used throughout:

- Only public, self-authored material was used: `tests/fixtures/corpus.json`,
  `tests/fixtures/coverage.json`, [grammar-coverage.md](grammar-coverage.md), this
  repository's tests, and small self-authored shapes run through the release binary
  built from `a7f2191` (the `main` commit this triage was written against).
- The private reference corpus was not accessed, requested or inferred. Every
  number that came from it (for example "about 16 files") appears below only as a
  quoted claim and is marked **ledger-only**.
- The public corpus this document refers to is the one `tests/docs-drift.rs` checks.
  Corpus counts: 151 cases total, 121 golden and 30 refusal.
- "Observed" means: the shape was piped to `verse-fmt -` on that build and the exit
  code was recorded. It is a parser, guard and formatter result, **not UEFN compiler
  acceptance**. A UEFN build is human-gated and has not been run for any row.
- The coverage plan and the diagnosis results are kept outside this repository. Their
  stated rules, as quoted in the work item for this triage, are: do not implement the
  top three patterns at once; do not choose broad scanner or precedence changes on
  frequency alone; do not guess to fill a list shorter than three candidates. The
  ranking criteria used here (safety impact, evidence strength, dependency, file
  count, change breadth) are the same list. The plan text itself was not re-read for
  this document.

## The nine issues are closed duplicates

All nine issues named in the work item (`#32`, `#33`, `#35` to `#41`) are **closed**
on GitHub, each with the comment "closed as a duplicate (same-day batch re-creation)".
The open originals are `#3`, `#4`, `#6` to `#12`. Matching by body text, the pairs are:

| Cited issue | Open original | Topic |
|---|---|---|
| `#32` | `#3` | P2 colon `if:` multi-condition |
| `#33` | `#4` | P3 case arm multi-statement body |
| `#35` | `#6` | D1 declaration / outer body failures |
| `#36` | `#7` | low-count six patterns |
| `#37` | `#8` | map literal policy |
| `#38` | `#9` | top-level inline binary split |
| `#39` | `#10` | lexical vs CST block comment |
| `#40` | `#11` | P2 gap: block comment line between conditions |
| `#41` | `#12` | P2 gap: bad-dedent second condition |

The closing comments name issue numbers one lower than the content match (`#32` says
`#2`, but `#2` is the re-measurement task and `#3` carries the P2 text). Whoever
tidies the tracker should confirm the mapping; this document follows the body text.
Related, but outside the nine: `#5`/`#34` (diagnostics stage separation) is already
implemented and tested in `tests/diagnostics-stage.rs`; `#2` is the ledger
re-measurement and is the human request in the last section.

## Main moved after these issues were written

The issues describe a `main` from early autumn. Since then the following merged, and
`grammar-coverage.md` records each as a supported row with a fixture and a test. This
changes the recommendation for most entries below, so it comes first.

| Form | Observed on `a7f2191` | Pinned by |
|---|---|---|
| Newline-separated `if:` condition block | exit 0 | corpus `if-block-multi-condition` |
| `case` arm with a multi-statement block | exit 0 | corpus `case-arm-block-bodies` |
| Indented `spawn:` with one expression | exit 0 | corpus `indented-spawn` |
| Indented enum with variant commas | exit 0 | corpus `indented-enum-commas` |
| Constructor declaration, empty constructor body | exit 0 | corpus `constructor-declaration` |
| Nested generic `[]?int`, `[][]int` | exit 0 | coverage row for array and `logic` type arguments |
| Map literal, string key, non-empty | exit 0 | corpus `map-literal-string-key`, `map-literal-pairs` |
| Top-level inline function body with operators | exit 0 | corpus `inline-binary-body` |

All of these are formatter and parser coverage, not UEFN acceptance. The issues'
claim that these shapes exit 2 can be true only of an older build or of a private
file whose shape differs from the self-authored one; that cannot be told apart
without the ledger.

## Entries

### #3 (cited as #32): P2 colon `if:` with newline-separated conditions

- **Claim (not verified):** the grammar's `if_condition` only takes comma-separated
  items, so a Verse file with newline-only separation is rejected; about 16 files.
  The 16 is **ledger-only**.
- **Checkable publicly:** observed exit 0 for two `A := 1` / `B := 2` conditions on
  separate lines, for a condition continued over a call, and for comma-joined
  conditions. Observed exit 2 for two conditions on one line (adjacent-statement
  guard) and for a second condition indented deeper than the first (structure
  guard). The one-per-line form is pinned by corpus `if-block-multi-condition` and
  `src/syntax.rs::indented_if_condition_keeps_each_line_as_its_own_item`.
- **Ledger-only (blocked):** whether the files that motivated the issue still fail on
  current `main`, and with which shape. A public repro that fails today was not
  found.
- **Self-authored repro:** none that fails today. If the ledger still shows failures,
  the next shape to try is a condition block whose items mix a continued call line
  and a trailing `then:` on the same dedent level; that is a hypothesis, not a
  finding.
- **Safety risk:** high for any further change. It touches layout and the structure
  guard; the plan conditions at stake are "no accepted illegal dedent" and "comment
  boundary stays consistent". A comma-optional grammar change is already ruled out
  by the issue itself.
- **Recommendation:** `needs private ledger access (human)`. Re-measure first; the
  public evidence says the headline shape is already supported.

### #4 (cited as #33): P3 case arm with more than one statement

- **Claim (not verified):** `case_arm` body is a single expression; two statements in
  one arm fail; about 8 files (**ledger-only**).
- **Checkable publicly:** observed exit 0 for an arm holding two statements and for
  an arm holding a binding then a call; exit 0 for inline arms. Observed exit 2 for a
  second statement dedented to a non-sibling column (a parser refusal). Pinned by
  corpus `case-arm-block-bodies` and `src/syntax.rs::case_arm_bodies_keep_their_statements`.
- **Ledger-only (blocked):** whether any private file still fails, and in which arm
  shape.
- **Self-authored repro:** none that fails today. The bad-dedent refusal above has no
  named test of its own.
- **Safety risk:** medium. Arm ownership, dedent and next-arm capture are the
  threatened conditions ("no accepted illegal dedent", formatter layout stays
  CST-owned). The existing shallower-wildcard guards show how easily arm boundaries
  go wrong.
- **Recommendation:** `needs private ledger access (human)`.

### #6 (cited as #35): D1 declaration and outer-body failures

- **Claim (not verified):** eight files fail in declaration or outer-body position and
  the cause layer is not identified (**ledger-only**).
- **Checkable publicly:** nothing specific. Self-authored class, method, using and
  `where` shapes (class with field bodies, `using` after code, a `where` parameter)
  all exit 0.
- **Ledger-only (blocked):** the whole item. The issue's method (per-file stop stage,
  raw ERROR count, candidate, confidence) is a private-ledger procedure.
- **Self-authored repro:** none, and none can be invented without the ledger; an
  identifier-substituted copy is explicitly not acceptable.
- **Safety risk:** unknown. The plan's own rule is not to force an unconfirmed
  failure into one of the three named patterns, so it must not be scoped to a
  grammar area yet.
- **Recommendation:** `needs private ledger access (human)`.

### #7 (cited as #36): six low-count patterns

- **Claim (not verified):** one hypothesis each for `spawn` colon, nested generic,
  indented enum comma, constructor declaration, empty constructor body, map entry.
- **Checkable publicly:** all six observed exit 0 in their simplest self-authored
  shape. Each is a supported row (see the table above). A `spawn:` block with two
  expressions is refused (exit 2, parser stage); `indented-spawn` covers one.
- **Ledger-only (blocked):** whether the private occurrences use a shape different
  from the six above.
- **Self-authored repro:** the six hypotheses do not reproduce. The one public shape
  that fails is `spawn:` with two expressions, which is a different claim from the
  one in the issue; whether it matters is **ledger-only**.
- **Safety risk:** low per shape, but the issue's own caution applies: low frequency
  is no reason for a scanner or precedence change.
- **Recommendation:** `needs private ledger access (human)`.

### #8 (cited as #37): map literal policy

- **Claim (not verified):** map literal is unsupported and kept as a refusal fixture.
  That statement is now out of date.
- **Checkable publicly:** observed exit 0 for `map{"x" => 1}`, the empty `map{}`,
  integer keys, a nested array value, and a map as a call argument. Pinned by corpus
  `map-literal-string-key` and `map-literal-pairs`. Observed exit 2 for the indented
  `map:` form with `=>` entries (adjacent-statement guard).
- **Ledger-only / human-gated:** whether UEFN accepts the indented `map:` form at
  all. No official reference is recorded for it (the coverage table's Reference
  column says `none recorded` for every row).
- **Self-authored repro:** the braced form is already a golden. The indented form
  needs a pinned refusal fixture and, if it is ever adopted, a UEFN build result
  first.
- **Safety risk:** medium for the indented form (adjacent-statement guard, and the
  rule "never turn an ambiguous parse into success"); the braced form is done.
- **Permanent fail-closed?** The evidence does **not** support it for the braced
  form, which is supported. For the indented form the guard already refuses it
  safely today, which is a fail-closed outcome, but making it permanent needs the
  human UEFN answer above.
- **Recommendation:** `needs self-authored repro first` (pin the indented-form
  refusal, then decide with the UEFN result).

### #9 (cited as #38): top-level inline binary function split

- **Claim (not verified):** a top-level single-line binary function can be split into
  separate nodes without an ERROR, which would let format or fix change meaning.
- **Checkable publicly:** observed exit 0 for `Add(X:int,Y:int):int = X+Y`, with
  call operands, and followed by another definition; the body owns the whole
  expression (corpus `inline-binary-body`,
  `src/syntax.rs::an_inline_function_body_takes_the_whole_expression`). Observed
  exit 2 for a statement line that starts with unary `+`/`-` after an expression
  (corpus `reject-leading-unary-continuation`) and for unseparated statements on one
  line (corpus `reject-unseparated-statements`).
- **Ledger-only (blocked):** the "adjacent 10 files" overlap the issue mentions
  (**ledger-only**).
- **Self-authored repro:** the dangerous shape is covered by existing refusals; no
  new repro is needed.
- **Safety risk:** this is the item that most threatens the "format never changes
  meaning" condition, which is why the guards matter more than the parse.
- **Permanent fail-closed?** Yes, for the residual ambiguity: a line-leading unary
  operator and a line with unseparated statements stay refused with a clear
  diagnostic. The parseable inline binary body is supported and should stay so.
- **Recommendation:** `leave as permanent fail-closed` (for the residual ambiguous
  shapes only; nothing to implement).

### #10 (cited as #39): lexical block comment vs CST `block_comment`

- **Claim (not verified):** the comment-boundary guard only rejects a mismatch;
  the underlying alignment between the lexer and the CST is still open.
- **Checkable publicly:** the guard exists and is tested
  (`tests/comment-boundary.rs`). Block comments after code, joined statements across
  a comment, and a comment hiding a line break before `(` are covered by
  `block-comments-mid-file`, `reject-comment-joined-statements` and
  `reject-opener-after-hidden-line-break`. Observed exit 0 for a block comment inside
  an expression, one spanning a line inside call arguments, and one on its own line.
- **Ledger-only (blocked):** any private file whose comment shape still trips the
  guard.
- **Self-authored repro:** none that shows a remaining lexer/CST disagreement. The
  smallest shape that would demonstrate one is a comment whose interior newline sits
  next to a binding in a condition block (the shape the issue names).
- **Safety risk:** high. Scanner and `extras` changes are the broad-change area the
  plan warns against; the threatened condition is "comment boundary stays
  consistent", and the guard must not be loosened.
- **Recommendation:** `needs self-authored repro first`.

### #11 (cited as #40): block comment line between `if:` conditions

- **Claim (not verified):** baseline and prototype both ERROR; fix the expectation at
  exit 2 until a grammar slice lands.
- **Checkable publicly:** observed **exit 0**, with output byte-identical to input
  and idempotent, for `A := 1`, an own-line `<# note #>`, then `B := 2` inside an
  `if:` block. The claim that main exits 2 is stale. No corpus case or test pins this
  shape (searching `corpus.json` finds no `if-` case with a comment line).
- **Ledger-only:** none.
- **Self-authored repro:** exists today and is the shape above. It needs a golden
  corpus case, not a refusal.
- **Safety risk:** medium. The threatened condition is "comment boundary stays
  consistent": a comment line between two conditions must not join them.
- **Recommendation:** `needs self-authored repro first` (add the golden; a corpus
  change moves `corpusSha256` and needs the parity update in `CONTRIBUTING.md`).

### #12 (cited as #41): second condition at a bad dedent

- **Claim (not verified):** a prototype accepted a second condition dedented too far;
  production must keep refusing it.
- **Checkable publicly:** observed exit 2 (`unsupported or incomplete Verse syntax`,
  stage parser, class parse) for a second condition dedented two columns. A
  second condition indented deeper is also exit 2 (stage structure). No corpus case
  names either as an `if:` refusal.
- **Ledger-only:** none.
- **Self-authored repro:** exists (above). It needs a pinned refusal.
- **Safety risk:** high if it ever regresses: the threatened condition is "no
  accepted illegal dedent". It is the required gate for any future P2 work.
- **Recommendation:** `needs self-authored repro first` (pin the refusal).

## Ranking

Criteria, in the plan's order: safety impact, evidence strength, dependency, file
count, change breadth. Frequency alone is not a criterion. File counts are
**ledger-only** claims and are used only as a tie-break where a note says so.

| Rank | Item | Why here | Needs ledger to justify? |
|---|---|---|---|
| 1 | #12 bad-dedent refusal | Highest safety impact; evidence is public and already reproduces; no grammar change; unblocks P2 as a gate. | No |
| 2 | #11 comment line between conditions | Public repro exists; pins current exit 0 behaviour before anything else can change it; no grammar change. | No |
| 3 | #8 indented map refusal fixture | Public repro; small; settles the policy question for the form that still fails. | No |
| 4 | #9 residual inline binary ambiguity | Already refused; nothing to build; kept for the record. | No |
| 5 | #10 lexer vs CST comment alignment | Evidence is weak (no failing public shape); broad scanner change. | No, but a repro is missing |
| 6 | #3, #4 (P2, P3) | Public evidence says they pass; whether anything still fails is unknown. | **Yes.** Their order against each other cannot be set without ledger counts. |
| 7 | #7 low-count patterns | Hypotheses do not reproduce; no dependency. | **Yes** |
| 8 | #6 declaration / outer body | No public evidence and no scope; cannot be ranked against the others. | **Yes** |

Ranks 1 to 5 can be justified from public evidence. The order of #3 against #4, and
the placement of #6 and #7 relative to them, needs ledger data and is deliberately
left unresolved rather than guessed.

## What a human must supply

- **Ledger access.** One request covers #3, #4, #6 and #7: on the release binary
  built from `main` at `a7f2191` or later, re-run the private reference-corpus
  read-only measurement (issue `#2`) and record anonymized counts only (no paths, no
  content, no identifiers) of, per file, the failing stage (lexer, parser, structure,
  guard) and which of these families it belongs to: newline-separated `if:`
  conditions, multi-statement case arm, declaration or outer body, other. Do not
  paste any of it into a public issue.
- **UEFN build.** For the indented `map:` form (#8) and, if wanted, `spawn:` with two
  expressions (#7): compile the self-authored shape against
  `++Fortnite+Release-42.20-CL-58011042-Windows` and record accept or reject, with the
  compiler message, in the UEFN column of the coverage table.

## Out of scope for this slice

No grammar, scanner, guard or test behaviour was changed. No issue or slot was
created, and the `[refusal]` issues were not touched. No crate publish, tag or release
is part of this change. Implementing any recommendation above, including adding the
corpus cases that ranks 1 to 3 call for, is a separate slice.
