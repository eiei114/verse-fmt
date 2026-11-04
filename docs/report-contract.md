# JSON report contract

`verse-fmt --output-format json` writes exactly one JSON object to stdout and
nothing else. This document is the canonical description of that envelope:
field meanings, path normalization, ordering and the stability promise. The
machine-readable form is [`diagnostics.schema.json`](diagnostics.schema.json)
(JSON Schema draft-07).

Text remains the default. `--output-format text` is byte-identical to omitting
the flag, for stdout, stderr and exit code alike.

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
only in `executionErrors` (with `path: null`) when it is not.

`--write --output-format json` is allowed and reports the post-write state, the
way the linter reports post-fix diagnostics. Successfully written files are
`formatted`, not `would-format`.

### `reason`

| Field | Meaning |
|---|---|
| `stage` | The pipeline stage that produced this status. |
| `message` | The diagnostic without the path prefix. For a positioned failure it starts with `line:column: `. |
| `offset` | Byte offset in the analyzed source, present when the failure has a known position. |

`reason.stage` uses a fixed vocabulary. Only stages this binary can actually
distinguish are populated; the rest are reserved so the field never has to be
reinterpreted later.

| Stage | Produced by | Populated today |
|---|---|---|
| `discovery` | Path guards, ignore/exclude evaluation, configuration resolution | No — reserved |
| `source` | Size, UTF-8, NUL, bare-CR and read/decode failures | Yes |
| `lexer` | Token scanner: unterminated or nested comment/string, unbalanced delimiters, character-literal, token and nesting limits | Yes |
| `parser` | Tree-sitter parse: ERROR/MISSING nodes, parse budget, ambiguous adjacent statements, lexical/CST block-comment disagreement | Yes |
| `structure` | Indentation analysis: tab indentation, unsupported continuation, inconsistent dedent, nesting depth | Yes |
| `layout` | Layout emission and the difference between source and output | Yes — also the `reason` of a `would-format`/`formatted` entry |
| `validation` | Post-format token/structure/idempotence checks | Defensive guard; no known input reaches it |
| `write` | Write preflight, staging and replacement | Reserved — a write failure is an `executionErrors` entry, not a refused file |
| `reporting` | Emitting human or machine output | No — reserved |

A content change is attributed to `layout` because that is the stage that
decided the bytes differ. This slice reports the stages the current pipeline can
distinguish; it does not split `parser` into narrower stages.

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

## Paths

`path` uses `/` separators and is relative to the configuration root, matching
`verse-lint`'s convention so paired tooling can join on path. The configuration
root is the directory containing the effective `verse.toml`, or the current
directory when there is none. For stdin the label is `<stdin>`, or the
`--stdin-filepath` value with `/` separators.

An entry never contains an absolute path. Names outside the root (via `..`) are
still emitted as the relative label the text path prints.

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

`--output-format json` combined with `--diff` or `--show-config` exits 2 with a
message on stderr and writes nothing to stdout: the diff is human text and
`--show-config` prints TOML, so neither can be wrapped in the envelope.

## Stability

Within `schemaVersion: 1`:

- existing properties keep their names, types and meanings;
- existing `status` and `stage` values are never removed or repurposed;
- new optional properties and new enum values may be added;
- the envelope stays a single object on stdout.

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
