# Resource limits and their tests

Every documented limit has an in-repository test. None is covered only by a
measurement recorded in project history: before this file, the walker-scale entry
and depth stress runs and the iteration-17 unit boundary checks existed in notes
but not in the suite, so a regression would have gone unnoticed.

The canonical wording is the resource-limit sentence in the maintainer vault's
`4_Project/OSS/verse-fmt/Docs/windows-toolchain-contract.md`; [`cli.md`](cli.md)
restates it for this repository. The literals themselves live in
`src/documented_limits.rs`, and [`grammar coverage`](grammar-coverage.md) says
which forms the build accepts.

## Documented values

`tests/docs-drift.rs` parses these `` `key = value` `` pairs from `cli.md`,
`parser-design.md` and this file and compares each one with the constant in
`src/documented_limits.rs`. A mismatch fails with the document, the claim and the
actual value. The coverage table below names each limit by its key rather than by
its value, and any other size or duration these three documents state is compared
with the limit its own line names, not merely with the set of documented values:
a restated number beside a checked pair cannot drift on its own, and a value
borrowed from another limit is rejected. Set `VERSE_FMT_CONTRACT_DOC` to the vault
contract to check its wording against the same values.

- `source-bytes = 8 MiB`
- `total-bytes = 64 MiB`
- `nesting = 256`
- `sources = 10,000`
- `visited-entries = 100,000`
- `directory-depth = 128`
- `directory-entries = 100,000`
- `config-bytes = 256 KiB`
- `discovery-errors = 100`
- `exclude-globs = 256`
- `exclude-glob-bytes = 4,096 bytes`
- `tokens = 250,000`
- `parse-budget = 2 seconds`
- `cst-depth = 512`
- `fingerprint-events = 1,000,000`

## Coverage

| Limit | Test | Technique |
|---|---|---|
| `source-bytes` | `tests/resource-limits.rs::source_byte_limit_is_enforced_at_eight_mib` | One real file sized at the byte limit, then one byte more |
| `total-bytes` | `tests/resource-limits.rs::total_input_limit_is_enforced_at_sixty_four_mib`, `tests/resource-limits.rs::a_source_ordered_after_the_total_input_limit_is_not_reported` | Real per-source inputs until the total is crossed, then one extra byte; a further eligible source sorted after the crossing file has no status |
| `nesting` | `tests/resource-limits.rs::lexical_nesting_limit_is_enforced_at_256` | Nested brackets at the limit, then one more |
| `sources` | `tests/resource-limits.rs::source_count_limit_is_enforced_at_10000` | Real files at the limit, then one more |
| `visited-entries` | `src/files.rs::the_last_permitted_visited_entry_and_depth_are_accepted`, `src/files.rs::visited_entry_limit_fails_before_accessing_another_path`, `src/files.rs::visited_entry_limit_stops_before_later_siblings` | Walker counter injected; no entries at that scale on disk |
| `directory-entries` | `src/files.rs::directory_entry_limit_stops_before_later_siblings` | The cap injected on a real directory; no entries at that scale on disk |
| `directory-depth` | `tests/resource-limits.rs::directory_depth_limit_is_enforced_at_128` | One hop below the limit, then one more |
| `config-bytes` (`verse.toml`) | `tests/resource-limits.rs::config_byte_limit_is_enforced_at_256_kib` | One real config file sized at the byte limit, then one byte more |
| `config-bytes` (`.gitignore`) | `tests/resource-limits.rs::gitignore_byte_limit_is_enforced_at_256_kib` | One real ignore file sized at the byte limit, then one byte more |
| `exclude-globs` | `tests/resource-limits.rs::exclude_glob_limits_are_enforced_at_256_globs_and_4096_bytes` | Patterns at the limit, then one more |
| `exclude-glob-bytes` | `tests/resource-limits.rs::exclude_glob_limits_are_enforced_at_256_globs_and_4096_bytes` | One pattern sized at the byte limit, then one byte more |
| `tokens` | `tests/resource-limits.rs::token_limit_is_enforced_at_250000` | Real tokens at the limit, then one more |
| `parse-budget` | `src/syntax.rs::the_parse_budget_is_two_seconds_and_is_compared_exactly` | Budget predicate invoked directly; no wall-clock test |
| `cst-depth` | `tests/resource-limits.rs::cst_depth_limit_is_enforced_at_512`, `src/syntax.rs::cst_depth_limit_is_512_and_accepts_its_own_boundary` | A chain at the measured depth, then one more; reduced seam for the comparison itself |
| `fingerprint-events` | `src/syntax.rs::fingerprint_event_limit_is_one_million_and_bounds_the_walk` | Reduced bound through the private `Limits` seam |
| `discovery-errors` | `tests/resource-limits.rs::discovery_error_cap_is_enforced_at_100` | Missing inputs past the cap |

Each CLI-level test asserts the same four things for the failure case: exit 2,
the failure appears in `executionErrors` (an execution error, not a formatting
outcome), `complete: false` in the JSON envelope, and `--write` replaced nothing
(a whole-directory snapshot of bytes and mtimes is unchanged).

Heavy inputs are sized to stay usable on a normal runner. The two byte-limit cases
write their files themselves and take a few seconds; the source-count case creates
that many empty files. Limits that would need entry counts at the walker scale, a
million-node tree or the full parse budget are proven through a directly-invoked
predicate or a reduced constant, and each such test says so in its own comment.

## Documented vs actual

These are real differences between the documented behaviour and what this build
does, or behaviour that a reader would reasonably expect the documented sentence
to imply. No limit value was changed to make a test pass.

1. **Discovery limits stop traversal and retain earlier results.** When a
   visited-entry, depth, source-count or directory-entry limit is reached, the
   walk stops before visiting another sibling or explicit input. Sources found
   before the limit remain available for analysis and report their own status;
   later files are neither discovered nor analyzed. The
   `Discovery::limit_exceeded` flag distinguishes a limit from an unusable
   path. `tests/resource-limits.rs::directory_depth_limit_is_enforced_at_128`
   and `::source_count_limit_is_enforced_at_10000`, plus the walker tests in
   `src/files.rs`, cover partial results and stop behavior. The limit itself
   remains an invocation-level execution error (`path: null`, `complete: false`,
   exit 2), and any recorded refusal still stops the write phase, so a limited
   run writes nothing. A path-level discovery failure — a missing input, an
   unusable configuration, too many exclude globs — analyzes nothing on its own:
   while `Discovery::limit_exceeded` is false `src/main.rs` returns before
   analysis, so `files` stays empty. When a path-level failure and a limit occur
   in the same run the limit governs: sources found before it are still analyzed
   and report their own status, and the path-level failure remains its own
   `path: null` entry in `executionErrors`. Input order decides which of the two
   failures is reported at all, because the walk stops at the limit before it
   reaches a later explicit input.
   `tests/resource-limits.rs::a_path_error_alongside_a_discovery_limit_still_analyzes_the_rest`
   pins both orders. Per-source limits (`source-bytes`, `tokens`, `nesting`,
   `cst-depth`) keep the other sources' results. The `total-bytes` aggregate limit
   stops analysis, so it keeps only the sources analyzed before the crossing
   source: the crossing file itself is a `source` refusal, and a source ordered
   after it is never read, so it has no status at all — absent from `files` and
   from `filesChecked`. `tests/resource-limits.rs::total_input_limit_is_enforced_at_sixty_four_mib`
   and `::a_source_ordered_after_the_total_input_limit_is_not_reported` cover
   both shapes.
2. **The glob count and the glob byte size cannot be reached together.** The
   maximum number of globs, each at its maximum length, is more `verse.toml` than
   the configuration byte limit accepts, and that limit refuses first. The two
   documented limits interact, so the glob count and the glob byte size are
   tested separately.
3. **The fingerprint-event bound is checked on node entry only.** `shape` can
   exceed the configured event count by up to the tree depth before the refusal
   fires; the documented value is not reachable within the `tokens` limit, so the
   test pins the literal constant and proves the comparison through the reduced
   seam.
4. **The discovery-error cap is not a report-wide cap.** `src/files.rs` stops
   collecting discovery errors at `discovery-errors`, and an invocation-level
   error can still be appended, so a run with 150 missing inputs reports one
   entry more than the cap. The JSON report as a whole is not capped at that
   value: 200 unreadable or refused sources still produce 200 `executionErrors`
   entries.
5. **The CST depth bound counts every tree level the walk enters, not a
   tree-sitter "depth" reading.** `src/syntax.rs` compares its own `depth`
   counter (incremented on every `goto_first_child`, named node or not) against
   `cst-depth`, and refuses when the counter is already past it. The observable
   boundary pinned by the tests is therefore "510 chained additions accepted, 511
   refused", which measures walk depth one and two past the constant. Reading the
   constant as "CST nesting of that many nodes" would be wrong.
6. **A per-directory entry cap exists but is not documented in the shared
   contract sentence.** `src/files.rs` refuses a single directory with more direct
   entries than `directory-entries` (`MAX_DIRECTORY_ENTRIES`, "directory entry
   resource limit exceeded"). The contract sentence names a `visited-entries`
   limit across the walk, not a per-directory one. This repository states both
   values above so the drift guard covers the per-directory cap too.
   `src/files.rs::directory_entry_limit_stops_before_later_siblings`
   proves the boundary through the injected directory limit — a tiny cap on a
   real directory, so no entries at that scale are created — and it is listed in
   the table above.
7. **Indentation depth fails closed well before the documented nesting value.**
   `nesting = 256` bounds lexical/bracket nesting, but the scanner's indentation
   stack is capped at 253 entries so its serialized state fits the fixed buffer
   Tree-sitter reserves for it. The measured boundary is 252 levels accepted and
   253 refused: a 253-level indented source that passes the lexer is still
   refused by the parser, and a 255-level one by the adjacent-structure guard.
   `docs/grammar-coverage.md` records both halves as their own rows
   (`shallow-indentation` and `deep-indentation`), so the table states the
   accepted depth rather than implying that 256 indentation levels are accepted.
