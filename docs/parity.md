# Parity with verse-lint

`verse-fmt` and `verse-lint` are a pair. This document states this repository's
side of that obligation.

## Parity

- This repository produces the corpus (`tests/fixtures/corpus.json`) and the
  formatter binary.
- `verse-lint` owns and runs the paired harness (`scripts/verify-pair.py` in that
  repository). There is deliberately no pair harness here: a second copy would be
  a second source of truth.
- `parity.json` at this repository's root is the authoritative declaration of what
  the pair expects from this side: the shared toolchain contract version, the
  corpus path, the corpus SHA-256, the golden count, the refusal count and the
  grammar base revision the corpus is valid against.
- A grammar or corpus change requires updating `parity.json` here **and** the
  linter's own `parity.json` in the same change. The two declarations must agree
  once both land; the linter's copy follows this one.
- The formatter must never gain a runtime dependency on the linter, and the linter
  must never gain one on the formatter binary. Neither repository carries a
  submodule or a CI checkout of the other, and CI does not run the paired harness.
- The pinned contract version is shared and must move in both repositories
  together (see `cli.md` and `tests/contract-version.rs`).

The corpus the declaration describes, checked against the fixture by
`tests/docs-drift.rs`:

Corpus counts: 151 cases total, 121 golden and 30 refusal.

## The check

`scripts/verify-parity.ps1` validates `parity.json` against reality and is run by
`scripts/verify.ps1`. It needs no linter binary, no linter checkout and no
network access. It fails, naming the field, when:

- a field is missing, unexpected or malformed;
- the SHA-256 of `corpusPath` differs from `corpusSha256`;
- the golden or refusal count of the corpus differs from the declared one (a
  refusal is a case with `"reject": true`);
- `contractVersion` differs from `tests/fixtures/contract-pair.json` or from the
  sentence in `docs/cli.md`, the two sources `tests/contract-version.rs` compares;
- `formatterGrammarBaseRevision` is neither the revision recorded in
  `vendor/tree-sitter-verse/upstream.md` nor an ancestor of `HEAD`.

It does not claim convergence: only the linter's paired run proves that.

## Asymmetry with the linter's declaration

The field names `corpusSha256`, `goldenCaseCount` and `formatterGrammarBaseRevision`
are identical in both files and must hold the same values. The two declarations
differ on purpose:

- `contractVersion`, `corpusPath`, `refusalCaseCount` and `schemaVersion` exist
  only here, because this side produces the corpus.
- `lastVerifiedFormatterRevision` and `lastVerifiedLinterRevision` exist only in
  the linter, because they record the outcome of a paired run, which only the
  linter performs.
