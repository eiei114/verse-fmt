# Resource limits and their tests

Every documented limit has an in-repository test. None is covered only by a
measurement recorded in project history: before this file, the 100,000-entry and
128-depth stress runs and the iteration-17 unit boundary checks existed in notes
but not in the suite, so a regression would have gone unnoticed.

The canonical wording is the resource-limit sentence in the maintainer vault's
`4_Project/OSS/verse-fmt/Docs/windows-toolchain-contract.md`; [`cli.md`](cli.md)
restates it for this repository. Enforcement lives in `src/limits.rs`,
`src/files.rs`, `src/source.rs`, `src/lex.rs` and `src/syntax.rs`. The drift guard
in a follow-up slice compares the documented sentence against those constants.

## Coverage

| Limit | Test | Technique |
|---|---|---|
| 8 MiB per source | `tests/resource-limits.rs::source_byte_limit_is_enforced_at_eight_mib` | Real 8 MiB file, then one byte more |
| 64 MiB total input | `tests/resource-limits.rs::total_input_limit_is_enforced_at_sixty_four_mib`, `tests/resource-limits.rs::a_source_ordered_after_the_total_input_limit_is_not_reported` | Eight real 8 MiB files, then one extra byte; a further eligible source sorted after the crossing file has no status |
| nesting 256 | `tests/resource-limits.rs::lexical_nesting_limit_is_enforced_at_256` | 256 nested `(`, then 257 |
| 10,000 sources | `tests/resource-limits.rs::source_count_limit_is_enforced_at_10000` | 10,000 real files, then 10,001 |
| 100,000 visited entries | `src/files.rs::the_last_permitted_visited_entry_and_depth_are_accepted`, `src/files.rs::visited_entry_limit_fails_before_accessing_another_path`, `src/files.rs::visited_entry_limit_stops_before_later_siblings` | Walker counter injected; no 100,000 entries on disk |
| 100,000 per-directory entries | `src/files.rs::directory_entry_limit_stops_before_later_siblings` | Directory limit injected as a 2-entry cap on a real directory; no 100,000 entries on disk |
| directory depth 128 | `tests/resource-limits.rs::directory_depth_limit_is_enforced_at_128` | 127 hops, then 128 hops |
| 256 KiB `verse.toml` | `tests/resource-limits.rs::config_byte_limit_is_enforced_at_256_kib` | Real 256 KiB file, then one byte more |
| 256 KiB `.gitignore` | `tests/resource-limits.rs::gitignore_byte_limit_is_enforced_at_256_kib` | Real 256 KiB file, then one byte more |
| 256 exclude globs | `tests/resource-limits.rs::exclude_glob_limits_are_enforced_at_256_globs_and_4096_bytes` | 256 patterns, then 257 |
| 4,096-byte glob | `tests/resource-limits.rs::exclude_glob_limits_are_enforced_at_256_globs_and_4096_bytes` | One 4,096-byte pattern, then 4,097 |
| 250,000 lexical tokens | `tests/resource-limits.rs::token_limit_is_enforced_at_250000` | 250,000 real tokens, then 250,001 |
| 2-second parse budget | `src/syntax.rs::the_parse_budget_is_two_seconds_and_is_compared_exactly` | Budget predicate invoked directly; no wall-clock test |
| CST depth 512 | `tests/resource-limits.rs::cst_depth_limit_is_enforced_at_512`, `src/syntax.rs::cst_depth_limit_is_512_and_accepts_its_own_boundary` | Chain measuring depth 512, then 513; reduced seam for the comparison itself |
| 1,000,000 fingerprint events | `src/syntax.rs::fingerprint_event_limit_is_one_million_and_bounds_the_walk` | Reduced bound through the private `Limits` seam |
| 100 discovery errors | `tests/resource-limits.rs::discovery_error_cap_is_enforced_at_100` | 100 missing inputs, then 150 |

Each CLI-level test asserts the same four things for the failure case: exit 2,
the failure appears in `executionErrors` (an execution error, not a formatting
outcome), `complete: false` in the JSON envelope, and `--write` replaced nothing
(a whole-directory snapshot of bytes and mtimes is unchanged).

Heavy inputs are sized to stay usable on a normal runner. The 8 MiB and 64 MiB
cases write their files themselves and take a few seconds; the 10,000-source case
creates 10,000 empty files. Limits that would need 100,000 real entries, a
million-node tree or a two-second parse are proven through a directly-invoked
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
   pins both orders. Per-source limits (8 MiB, token, nesting, CST depth) keep
   the other sources' results. The 64 MiB aggregate limit stops analysis, so it
   keeps only the sources analyzed before the crossing source: the crossing file
   itself is a `source` refusal, and a source ordered after it is never read, so
   it has no status at all — absent from `files` and from `filesChecked`.
   `tests/resource-limits.rs::total_input_limit_is_enforced_at_sixty_four_mib`
   and `::a_source_ordered_after_the_total_input_limit_is_not_reported` cover
   both shapes.
2. **256 globs of 4,096 bytes cannot be reached together.** 256 × 4,096 bytes is
   1 MiB of `verse.toml`, which the 256 KiB configuration limit refuses first. The
   two documented limits interact, so the glob count and the glob byte size are
   tested separately.
3. **The fingerprint-event bound is checked on node entry only.** `shape` can
   exceed the configured event count by up to the tree depth before the refusal
   fires; the documented value 1,000,000 is not reachable within the 250,000-token
   limit, so the test pins the literal constant and proves the comparison through
   the reduced seam.
4. **The 100-execution-error cap is a discovery-diagnostic cap.** `src/files.rs`
   stops collecting discovery errors at 100, and an invocation-level error can
   still be appended, so a run with 150 missing inputs reports 101 entries. The
   JSON report as a whole is not capped at 100 errors: 200 unreadable or refused
   sources still produce 200 `executionErrors` entries.
5. **The CST depth bound counts every tree level the walk enters, not a
   tree-sitter "depth" reading.** `src/syntax.rs` compares its own `depth`
   counter (incremented on every `goto_first_child`, named node or not) against
   512, and refuses when the counter is already past it. The observable boundary
   pinned by the tests is therefore "510 chained additions accepted, 511 refused",
   which measures walk depth 512 and 513. Reading the constant as "CST nesting of
   512 nodes" would be wrong.
6. **A per-directory entry cap exists but is not documented.** `src/files.rs`
   refuses a single directory with more than 100,000 direct entries
   (`MAX_DIRECTORY_ENTRIES`, "directory entry resource limit exceeded"). The
   contract sentence names a 100,000 *visited entry* limit across the walk, not a
   per-directory one. `src/files.rs::directory_entry_limit_stops_before_later_siblings`
   proves the boundary through the injected directory limit — a 2-entry cap on a
   real directory, so no 100,000 real entries are created — and it is listed in
   the table above. It stays in this section because the contract sentence does
   not name a per-directory cap, not because it is untested.
