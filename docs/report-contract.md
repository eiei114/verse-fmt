# JSON report contract

`verse-fmt --output-format json` writes exactly one JSON object to stdout and
nothing else. This document is the canonical description of that envelope:
field meanings, path normalization, ordering and the stability promise. The
machine-readable form is [`diagnostics.schema.json`](diagnostics.schema.json)
(JSON Schema draft-07).

Text remains the default. `--output-format text` is byte-identical to omitting
the flag, for stdout, stderr and exit code alike.

Text diagnostics for a refusal now end with a stable stage marker, `[stage=…]`,
and a guard refusal also carries `[class=guard]` plus one `[class=parse]` line
for the underlying parse error. `--explain` adds the executed stage order and
the complete failure chain; without it the diagnostic stays concise and keeps
its previous wording. See [CLI](cli.md#stage-markers-and---explain).

## Envelope

```json
{
  "schemaVersion": 1,
  "tool": { "name": "verse-fmt", "version": "0.1.0-alpha.1" },
  "summary": {
    "filesChecked": 3,
    "filesChanged": 1,
    "filesUnchanged": 1,
    "filesRefused": 1,
    "complete": false
  },
  "files": [
    { "path": "src/ok.verse", "status": "unchanged" },
    {
      "path": "src/needs.verse",
      "status": "would-format",
      "reason": { "stage": "layout", "message": "formatter output differs from the source" }
    },
    {
      "path": "src/broken.verse",
      "status": "refused",
      "reason": {
        "stage": "lexer",
        "message": "1:1: unterminated block comment",
        "offset": 0
      }
    },
    {
      "path": "src/guarded.verse",
      "status": "refused",
      "reason": {
        "stage": "parser",
        "class": "guard",
        "message": "1:35: ambiguous adjacent top-level statements; inline binary function bodies are unsupported by the pinned grammar",
        "offset": 34,
        "underlying": [
          {
            "stage": "parser",
            "class": "parse",
            "offset": 49,
            "nodeKind": "ERROR",
            "message": "underlying parse error: ERROR node at byte 49; unsupported or incomplete Verse syntax"
          }
        ]
      }
    }
  ],
  "executionErrors": [
    { "path": "src/broken.verse", "message": "1:1: unterminated block comment" }
  ]
}
```

Properties are emitted in the order shown. New optional properties may be added
without a `schemaVersion` increment; consumers must ignore unknown properties
inside a known major version and must branch on `schemaVersion` before relying
on any field.

### `tool`

`name` is always `verse-fmt`. `version` is the crate version of the binary that
produced the report. Both are stable per build; neither reveals machine state.

### `summary`

| Field | Meaning |
|---|---|
| `filesChecked` | Number of entries in `files`. Identical to the count of analyzed sources. |
| `filesChanged` | `formatted` plus `would-format` entries. |
| `filesUnchanged` | `unchanged` entries. |
| `filesRefused` | `refused` entries. |
| `complete` | `false` exactly when `executionErrors` is non-empty, i.e. the invocation failed as a whole and exited 2. |

`filesChanged` is not a success signal: a refusal alongside a pending change
still exits 2, and `complete` is how a consumer detects that.

### `files`

One entry per analyzed source, in the order the sources were analyzed. Directory
discovery sorts by relative path, so recursive runs are ordered by `path`.

| `status` | When it is reported | Exit contribution |
|---|---|---|
| `unchanged` | The formatter produced output equal to the source bytes. | 0 |
| `would-format` | `--check` (or plain formatting to stdout with `--output-format json`) found a difference, and nothing was written. | 1 when combined with `--check`/`--diff` |
| `formatted` | `--write` replaced the file with the formatted bytes, and that file's own commit returned success. | 0 |
| `refused` | Analysis of this source failed. `reason.stage` says where. | Always 2 |

`unchanged` entries carry no `reason`. Every other status carries one.

A change is `would-format` until its bytes are on disk. `--write` promotes an
entry to `formatted` only after that entry's replacement committed, so a batch
that stops partway — a staging failure before any commit, or a commit failure
mid-batch — leaves every file that was never replaced as `would-format`. The
report therefore never claims a file was written when it was not. Files that
*had* already committed before the failure keep `formatted`: the status
describes each file's own outcome, and the batch does not roll back completed
work.

A source that fails discovery or cannot be read is not silently dropped: it is
reported as `refused` when the failure is attributable to one path, and appears
only in `executionErrors` (with `path: null`) when it is not. A discovery *limit*
(visited entries, directory depth, source count, per-directory entries) also
produces a `path: null` execution error, but the sources discovered before the
limit still appear in `files` with their own status — `filesChecked` counts them
even though `complete` is `false` and the exit code is 2. See
[resource limits](resource-limits.md).

`--write --output-format json` is allowed and reports the post-write state, the
way the linter reports post-fix diagnostics. Successfully written files are
`formatted`, not `would-format`.

### `reason`

| Field | Meaning |
|---|---|
| `stage` | The pipeline stage that produced this status. |
| `message` | The diagnostic without the path prefix. For a positioned failure it starts with `line:column: `. |
| `offset` | Byte offset in the analyzed source, present when the failure has a known position. |
| `class` | Optional. Present only where one stage has two distinguishable origins: `guard` for the adjacent-structure refusal, `parse` for a raw CST `ERROR`/`MISSING` finding. Both are `stage: "parser"`. |
| `underlying` | Optional array. Failures recorded *after* the primary one that stopped processing. Only a guard refusal carries entries today. |

`reason.stage` uses a fixed vocabulary. Only stages this binary can actually
populate are listed as populated; the rest are reserved so the field never has
to be reinterpreted later.

| Stage | Produced by | Populated today |
|---|---|---|
| `discovery` | Path guards, ignore/exclude evaluation, configuration resolution | As an `executionErrors` entry, never as a `refused` file — discovery fails before a source exists |
| `source` | Size, UTF-8, NUL, bare-CR and read/decode failures | Yes |
| `lexer` | Token scanner: unterminated or nested comment/string, unbalanced delimiters, character-literal, token and nesting limits | Yes |
| `parser` | Tree-sitter parse: ERROR/MISSING nodes, parse budget, ambiguous adjacent statements, lexical/CST block-comment disagreement | Yes — also the `class` split described below |
| `structure` | Indentation analysis: tab indentation, unsupported continuation, inconsistent dedent, nesting depth | Yes |
| `layout` | Layout emission and the difference between source and output | Yes — also the `reason` of a `would-format`/`formatted` entry |
| `validation` | Post-format token/structure/idempotence checks | Defensive guard; no known input reaches it |
| `write` | Write preflight, staging and replacement | As an `executionErrors` entry, never as a refused file |
| `reporting` | Emitting human or machine output | No — reserved |

The text path names these stages even where the machine report has no `files`
entry for them: a discovery, read or write failure prints a `[stage=…]` marker on
stderr, and the machine copy stays a `path`/`message` `executionErrors` entry.
`reporting` is the one stage that only ever appears on stderr, when writing the
envelope itself fails.

A content change is attributed to `layout` because that is the stage that
decided the bytes differ.

### Primary and secondary failures

A `refused` entry reports the failure that **stopped processing** as its primary
reason — its `stage`, `class`, `message` and `offset`. Stages that stopped
running because of it are not reported as if they had run.

`class` exists because `parser` alone conflates two different outcomes, and a
reader who fixes one would otherwise meet the other afterwards:

- `class: "guard"` — the **adjoining-structure guard** refused the source. It
  runs before the ERROR/MISSING walk and is the primary refusal for every source
  it covers. Its message is a guard outcome, not a parse outcome.
- `class: "parse"` — the bounded CST walk found a raw `ERROR` or `MISSING` node
  and refused the source. With no guard refusal this is the primary reason.

When the guard refuses a source that also contains a raw parse error, the
guard stays primary and the parse error is attached as `underlying`: a raw
`ERROR` is never treated as evidence that the guard is wrong, and a guarded
source never becomes accepted because the reporting improved. A guard refusal
with no raw parse error carries no `underlying` entry at all; no secondary
failure is fabricated.

Every `underlying` entry names its own `stage`, `class`, `offset` and the
tree-sitter `nodeKind` (`ERROR`, or `MISSING <kind>` for a zero-width node the
grammar inserted), so a consumer can tell what the source *also* violates
without parsing the message prose.

### `executionErrors`

`path` and `message` mirror what the text path prints on stderr, one entry per
diagnostic. `path` is the affected source, or `null` when the failure is not
attributable to one source (a discovery failure, an unusable configuration).

A write failure — staging, preflight or replacement — is *only* an
executionErrors entry. It is never a `refused` file: a refusal means the source
was analyzed and rejected, and no `files` entry status changes because a write
failed. The affected file keeps whatever status its analysis produced, so
`would-format` plus a non-empty `executionErrors` is exactly "this file needed
changes and the write did not happen".

`message` never contains an absolute path. The text diagnostic keeps its host
spelling — it still prints the full path stderr always has — but the machine
copy rewrites each absolute spelling this invocation could have printed (the
configuration root and its ancestors, in native, `/`-separated and
`Debug`-escaped form) into a `/`-separated label relative to the configuration
root, the same way `path` is normalized. A path the root cannot relativize, such
as a target on another Windows volume, appears as `<abs>/…` rather than
leaking the absolute prefix. Only the path is rewritten; the wording around it
is unchanged, so a machine message is the stderr message with paths relativized.

Execution errors continue to go to stderr as well. Machine output never replaces
them: stdout carries the envelope, stderr carries the human diagnostic. Only the
envelope is machine-stable; the stderr wording is not part of this contract.

The text copy of a refusal carries the `[stage=…]` marker and any
`[class=…]` marker; the machine `message` does not, so a consumer never has to
strip a marker. `executionErrors[].message` is therefore the primary diagnostic
without the path prefix and without the markers, and it deliberately does **not**
repeat the underlying parse error: that belongs to `files[].reason.underlying`.

## Paths

`path` uses `/` separators and is relative to the configuration root, matching
`verse-lint`'s convention so paired tooling can join on path. The configuration
root is the directory containing the effective `verse.toml`, or the current
directory when there is none. For stdin the label is `<stdin>`, or the
`--stdin-filepath` value with `/` separators.

An entry never contains an absolute path. Names outside the root (via `..`) are
still emitted as the relative label the text path prints. A label the text path
prints as an absolute path — an absolute `--stdin-filepath`, or a file the root
cannot relativize, such as one on another volume or reached through a UNC
alias — is rewritten the same way as `message`: relativized against the root
when possible, masked with `<abs>/` otherwise. The text path is unchanged.

## Determinism

The same input, arguments and working directory produce identical bytes:

- no timestamps, durations or clock values;
- no absolute paths, host names or process identifiers;
- no hashes or fingerprints of machine state;
- no locale-dependent ordering — entries follow the sorted discovery order;
- no ANSI escapes, progress text, banners or human summaries, even with
  `--color always`.

## Exit codes

Unchanged by this format. `--output-format json` never becomes a success signal
by itself:

| Condition | Exit |
|---|---|
| Success, or `--write` completed | 0 |
| `--check`/`--diff` found pending changes | 1 |
| Any refusal, discovery/config failure, write failure, or an invalid flag combination | 2 |

`--output-format json` combined with `--diff`, `--show-config` or `--explain`
exits 2 with a message on stderr and writes nothing to stdout: the diff is human
text, `--show-config` prints TOML, and `--explain` is a human failure chain the
envelope already carries as `reason.stage`, `reason.class` and
`reason.underlying`.

## Stability

Within `schemaVersion: 1`:

- existing properties keep their names, types and meanings;
- existing `status`, `stage` and `class` values are never removed or
  repurposed;
- new optional properties and new enum values may be added;
- the envelope stays a single object on stdout.

`class` and `underlying` were added as optional properties in this version, so a
consumer that ignores them still reads the same envelope: a guard refusal keeps
`stage: "parser"` and the `refused` status it always had.

A breaking change — removing or renaming a property, changing a type, changing
what `complete` or an existing `status` means — increments `schemaVersion`.
`docs/diagnostics.schema.json` is updated in the same change, and
`cargo test` fails when the committed schema and the binary disagree.

## Not included

- **SARIF.** A formatter's output is not a diagnostic set in SARIF's sense. Do
  not expect it, and do not add it for symmetry with the linter.
- **Formatted source.** The envelope reports status; formatted bytes stay where
  they are today — stdout for the plain text mode, the file for `--write`.
- **Diff content.** Use `--diff` for a human diff; there is no machine field for
  it.
