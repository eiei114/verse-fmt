//! Documentation drift guard.
//!
//! Four documented surfaces are checked here against the code, fixtures and the
//! sibling repository they describe: the corpus counts, every resource limit, the
//! grammar-coverage table and the shared contract version. A mismatch fails with
//! the document, the claim and the actual value, so the fix is obvious.
//!
//! A value restated in prose is guarded too, not only its `` `key = value` `` pair:
//! the corpus counts may appear on the machine-checked claim line and nowhere
//! else, a paragraph that names the corpus attributes every count in it, every
//! size or duration in the three documents that carry the limit contract must
//! equal the limit named on its own line, and the coverage table names each limit
//! by its key. Where a sentence used to repeat a documented number beside the
//! pair, it now names the pair instead, so no unchecked copy of a documented
//! value is left in those documents.
//!
//! The limit literals come from `src/documented_limits.rs`, the same file the
//! binary uses, so a changed limit cannot disagree with a document silently.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[path = "../src/documented_limits.rs"]
mod documented_limits;

use documented_limits as limits;

/// The one shape a document may use to quote corpus counts.
const CORPUS_CLAIM: &str = "Corpus counts:";

const MIB: usize = 1024 * 1024;
const KIB: usize = 1024;

/// Limits the CLI document restates; the shared toolchain contract names the
/// same nine values.
const INPUT_KEYS: &[&str] = &[
    "source-bytes",
    "total-bytes",
    "nesting",
    "sources",
    "visited-entries",
    "directory-depth",
    "config-bytes",
    "exclude-globs",
    "exclude-glob-bytes",
];

/// Limits the parser document restates. `nesting` belongs here too: the parser
/// document states the lexical nesting pair the CLI document also carries.
const PARSER_KEYS: &[&str] = &[
    "tokens",
    "parse-budget",
    "cst-depth",
    "fingerprint-events",
    "nesting",
];

/// Every documented limit, including the two only the limit table names.
const LIMIT_KEYS: &[&str] = &[
    "source-bytes",
    "total-bytes",
    "nesting",
    "sources",
    "visited-entries",
    "directory-depth",
    "directory-entries",
    "config-bytes",
    "discovery-errors",
    "exclude-globs",
    "exclude-glob-bytes",
    "tokens",
    "parse-budget",
    "cst-depth",
    "fingerprint-events",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"))
}

/// `README.md`, `tests/fixtures/provenance.md` plus every Markdown document under
/// `docs/`, so a new document that quotes a number is checked without editing
/// this test. `tests/fixtures/provenance.md` carries the corpus claim line the
/// guard reads, and it was outside this list: a wrong count there left the
/// guard green.
fn documents() -> Vec<String> {
    let mut names = vec![
        "README.md".to_owned(),
        "tests/fixtures/provenance.md".to_owned(),
    ];
    for entry in fs::read_dir(root().join("docs")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".md") {
            names.push(format!("docs/{name}"));
        }
    }
    names.sort();
    names
}

// ---------------------------------------------------------------- corpus counts

#[derive(serde::Deserialize)]
struct Corpus {
    cases: Vec<CorpusCase>,
}

#[derive(serde::Deserialize)]
struct CorpusCase {
    #[serde(default)]
    reject: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counts {
    total: usize,
    golden: usize,
    refusal: usize,
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} cases total, {} golden and {} refusal",
            self.total, self.golden, self.refusal
        )
    }
}

fn corpus_counts() -> Counts {
    let corpus: Corpus = serde_json::from_str(&read("tests/fixtures/corpus.json"))
        .expect("tests/fixtures/corpus.json must stay parseable");
    let refusal = corpus.cases.iter().filter(|case| case.reject).count();
    Counts {
        total: corpus.cases.len(),
        golden: corpus.cases.len() - refusal,
        refusal,
    }
}

/// A document quotes corpus counts when it names the corpus and a count word;
/// such a document must also carry the machine-checkable claim line.
fn quotes_corpus_counts(text: &str) -> bool {
    text.contains("corpus.json")
        || (text.to_lowercase().contains("corpus")
            && (text.contains("golden") || text.contains("refusal")))
}

/// Phrasings a corpus count is written next to. A number beside one of these, on
/// a line that names the corpus, is a claim about `corpus.json`, so only the
/// machine-checked `Corpus counts:` line may carry the count: a second, prose
/// copy is exactly what the guard used to leave unchecked, because it read only
/// that one line.
const CORPUS_COUNT_PHRASES: &[&str] = &[
    "cases total",
    "cases",
    "golden",
    "refusal",
    "named inputs",
    "named cases",
    "named,",
    "self-authored named",
];

/// One document's corpus claim: the single `Corpus counts:` line must match the
/// fixture, and no other line may quote a corpus count. Both halves are needed —
/// the claim line alone let a prose copy drift, and a prose count alone would
/// carry no check at all.
fn check_corpus_document(name: &str, text: &str, counts: Counts) -> Result<(), String> {
    if !quotes_corpus_counts(text) {
        return Ok(());
    }
    let line = text
        .lines()
        .find(|line| line.contains(CORPUS_CLAIM))
        .ok_or_else(|| {
            format!(
                "{name} quotes corpus counts but has no `{CORPUS_CLAIM} <total> cases total, \
                 <golden> golden and <refusal> refusal` line; the corpus has {counts}"
            )
        })?;
    let claimed = Counts {
        total: number_before(line, "cases total").map_err(|error| {
            format!("{name}: claim line {line:?} is unreadable: {error}; the corpus has {counts}")
        })?,
        golden: number_before(line, "golden").map_err(|error| {
            format!("{name}: claim line {line:?} is unreadable: {error}; the corpus has {counts}")
        })?,
        refusal: number_before(line, "refusal").map_err(|error| {
            format!("{name}: claim line {line:?} is unreadable: {error}; the corpus has {counts}")
        })?,
    };
    if claimed != counts {
        return Err(format!(
            "{name} claims `{line}`; tests/fixtures/corpus.json actually holds {counts}"
        ));
    }
    // A Markdown paragraph names the corpus once, and a wrapped count lands on a
    // later line without the word: a per-line read skipped it, so a normal line
    // wrap concealed a stale count. Attribution therefore runs to the end of the
    // paragraph, whichever line carries the corpus word.
    for paragraph in paragraphs(text) {
        if !paragraph.iter().any(|piece| names_corpus(piece)) {
            continue;
        }
        let rest: Vec<&str> = paragraph
            .iter()
            .copied()
            .filter(|other| *other != line)
            .collect();
        // The lines of the paragraph are one sentence once the wrap is undone, so a
        // count split from its label ("contains 99" / "golden cases.") is read the
        // same way an unwrapped one is. Reading each line alone left that count
        // unguarded.
        let joined = rest.join(" ");
        for phrase in CORPUS_COUNT_PHRASES {
            if let Ok(number) = number_before(&joined, phrase) {
                return Err(format!(
                    "{name} quotes a second corpus count ({number} before {phrase:?}) in \
                     {joined:?}; the corpus has {counts}, and only the `{CORPUS_CLAIM}` line \
                     may carry a count, because that is the one line this guard reads"
                ));
            }
        }
    }
    Ok(())
}

fn names_corpus(line: &str) -> bool {
    line.to_lowercase().contains("corpus")
}

/// The document split into blank-line separated paragraphs. A count attributed to
/// the corpus on one line stays a corpus count for the whole paragraph, so a
/// wrapped sentence is read the same way as an unwrapped one.
fn paragraphs(text: &str) -> Vec<Vec<&str>> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(line);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The number written immediately before `label`, ignoring thousands separators.
/// Every occurrence of the label is tried, so a word that also appears earlier
/// in the sentence (`` `refusal` `` before the numbers) does not break the read.
fn number_before(segment: &str, label: &str) -> Result<usize, String> {
    let mut searched = 0;
    while let Some(offset) = segment[searched..].find(label) {
        let index = searched + offset;
        searched = index + label.len();
        let digits: String = segment[..index]
            .trim_end()
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_digit() || *ch == ',')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Ok(number) = digits.replace(',', "").parse() {
            return Ok(number);
        }
    }
    Err(format!("no number before {label:?} in {segment:?}"))
}

#[test]
fn every_document_quotes_the_actual_corpus_counts() {
    let counts = corpus_counts();
    let mut checked = Vec::new();
    for name in documents() {
        let text = read(&name);
        if let Err(error) = check_corpus_document(&name, &text, counts) {
            panic!("{error}");
        }
        if quotes_corpus_counts(&text) {
            checked.push(name);
        }
    }
    assert!(
        !checked.is_empty(),
        "no document quotes the corpus counts; README.md must state them explicitly"
    );
}

/// The prose copy the guard used to miss, pinned: a correct claim line with a
/// stale count somewhere else must fail, and a claim line that disagrees must
/// fail on its own.
#[test]
fn a_prose_copy_of_the_corpus_counts_fails_beside_a_claim_line() {
    let counts = Counts {
        total: 88,
        golden: 65,
        refusal: 23,
    };
    let good = "Corpus counts: 88 cases total, 65 golden and 23 refusal.\n";
    check_corpus_document("synthetic.md", good, counts).expect("the matching claim line must pass");
    let prose =
        format!("{good}`tests/fixtures/corpus.json` maps 87 named inputs to independent output.\n");
    let error = check_corpus_document("synthetic.md", &prose, counts)
        .expect_err("a prose count of 87 must not pass beside a claim line of 88");
    assert!(error.contains("87"), "{error}");
    // A wrapped claim: the corpus word stays on the first line and the stale count
    // lands on the next one, which is exactly how a normal Markdown wrap hides it
    // from a per-line read.
    let wrapped = format!("{good}The corpus contains:\n99 golden cases.\n");
    let error = check_corpus_document("synthetic.md", &wrapped, counts)
        .expect_err("a wrapped corpus count of 99 must not pass beside a claim line of 88");
    assert!(error.contains("99"), "{error}");
    // The same wrap with the correct counts is still a second copy: only the claim
    // line may carry a count.
    let wrapped_correct = format!("{good}The corpus contains:\n65 golden cases.\n");
    let error = check_corpus_document("synthetic.md", &wrapped_correct, counts)
        .expect_err("a wrapped prose copy must not pass even with the right numbers");
    assert!(error.contains("65"), "{error}");
    // A wrap that splits the number from its own label: neither line carries both,
    // so a per-line read saw nothing to reject.
    let split = format!("{good}The corpus contains 99\ngolden cases.\n");
    let error = check_corpus_document("synthetic.md", &split, counts)
        .expect_err("a count split from its label by a line wrap must still be read");
    assert!(error.contains("99"), "{error}");
    let stale = "Corpus counts: 87 cases total, 64 golden and 23 refusal.\n";
    let error = check_corpus_document("synthetic.md", stale, counts)
        .expect_err("a claim line that disagrees with the corpus must fail");
    assert!(error.contains("88 cases total"), "{error}");
}

/// A different corpus count is still a corpus count: the claim line is the one
/// line the guard reads, so a document that writes a count in prose only would
/// be unchecked rather than wrong.
#[test]
fn a_document_that_quotes_counts_in_prose_alone_must_state_the_claim_line() {
    let counts = Counts {
        total: 87,
        golden: 64,
        refusal: 23,
    };
    let prose = "The corpus holds 64 golden cases and 23 refusal cases.\n";
    let error = check_corpus_document("synthetic.md", prose, counts)
        .expect_err("prose counts without a claim line must fail");
    assert!(error.contains(CORPUS_CLAIM), "{error}");
}

// -------------------------------------------------------------- resource limits

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Value {
    Bytes(usize),
    Count(usize),
    Seconds(u64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(bytes) => write!(f, "{bytes} bytes"),
            Self::Count(count) => write!(f, "{count}"),
            Self::Seconds(seconds) => write!(f, "{seconds} seconds"),
        }
    }
}

fn constant(key: &str) -> Option<Value> {
    match key {
        "source-bytes" => Some(Value::Bytes(limits::MAX_SOURCE_BYTES)),
        "total-bytes" => Some(Value::Bytes(limits::MAX_TOTAL_BYTES)),
        "config-bytes" => Some(Value::Bytes(limits::CONFIG_BYTES)),
        "exclude-glob-bytes" => Some(Value::Bytes(limits::MAX_EXCLUDE_GLOB_BYTES)),
        "nesting" => Some(Value::Count(limits::MAX_NESTING)),
        "sources" => Some(Value::Count(limits::MAX_SOURCE_FILES)),
        "visited-entries" => Some(Value::Count(limits::MAX_VISITED_ENTRIES)),
        "directory-depth" => Some(Value::Count(limits::MAX_DIRECTORY_DEPTH)),
        "directory-entries" => Some(Value::Count(limits::MAX_DIRECTORY_ENTRIES)),
        "discovery-errors" => Some(Value::Count(limits::MAX_DISCOVERY_ERRORS)),
        "exclude-globs" => Some(Value::Count(limits::MAX_EXCLUDE_GLOBS)),
        "tokens" => Some(Value::Count(limits::MAX_TOKENS)),
        "cst-depth" => Some(Value::Count(limits::MAX_CST_DEPTH)),
        "fingerprint-events" => Some(Value::Count(limits::MAX_FINGERPRINT_EVENTS)),
        "parse-budget" => Some(Value::Seconds(limits::MAX_PARSE_BUDGET.as_secs())),
        _ => None,
    }
}

/// One documented limit in the only shape this guard reads: `` `key = value` ``.
fn documented_value(text: &str, key: &str) -> Option<String> {
    let needle = format!("`{key} = ");
    let start = text.find(&needle)? + needle.len();
    let end = text[start..].find('`')? + start;
    Some(text[start..end].to_owned())
}

fn parse_value(raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    let digits: String = raw
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == ',')
        .collect();
    if digits.is_empty() {
        return Err(format!("no number in {raw:?}"));
    }
    let number: usize = digits
        .replace(',', "")
        .parse()
        .map_err(|_| format!("unreadable number in {raw:?}"))?;
    let unit = raw[digits.len()..].trim();
    match unit {
        "" => Ok(Value::Count(number)),
        "bytes" => Ok(Value::Bytes(number)),
        "KiB" => Ok(Value::Bytes(number * KIB)),
        "MiB" => Ok(Value::Bytes(number * MIB)),
        "seconds" => Ok(Value::Seconds(number as u64)),
        other => Err(format!(
            "unknown unit {other:?} in {raw:?}; use bytes, KiB, MiB or seconds"
        )),
    }
}

fn compare_stated(name: &str, key: &str, raw: &str) {
    let expected = constant(key)
        .unwrap_or_else(|| panic!("{name} states `{key}`, which is not a documented limit"));
    let actual = parse_value(raw).unwrap_or_else(|error| {
        panic!("{name}: cannot read `{key} = {raw}`: {error}; the constant is {expected}")
    });
    assert_eq!(
        actual, expected,
        "{name} claims `{key} = {raw}`, but src/documented_limits.rs holds {expected}"
    );
}

/// Every size or duration a document states, with its unit and the character offset
/// the number starts at, so a quantity can be attributed to the limit named nearest
/// to it. Only an explicit unit counts, so a path, an offset or a plain count is not
/// read as a limit; `9-second` and `9 seconds` are both a duration, because a
/// measurement row may write either form and a change to `9-second` used to slip
/// past the unit scan entirely.
fn stated_sizes(text: &str) -> Vec<(usize, String, Value)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == ',') {
            index += 1;
        }
        let raw_number: String = chars[start..index].iter().collect();
        let mut after = index;
        while after < chars.len() && chars[after].is_whitespace() {
            after += 1;
        }
        // `9-second` writes a hyphen between the number and the unit; skip it
        // instead of stopping the unit scan there and dropping the quantity.
        if chars.get(after) == Some(&'-')
            && chars.get(after + 1).is_some_and(|ch| ch.is_alphabetic())
        {
            after += 1;
        }
        let mut unit_end = after;
        while unit_end < chars.len() && chars[unit_end].is_ascii_alphabetic() {
            unit_end += 1;
        }
        let unit: String = chars[after..unit_end].iter().collect();
        let number = raw_number.replace(',', "");
        let parsed = number.parse::<usize>().ok();
        let value = match (unit.as_str(), parsed) {
            ("MiB", Some(number)) => Some(Value::Bytes(number.saturating_mul(MIB))),
            ("KiB", Some(number)) => Some(Value::Bytes(number.saturating_mul(KIB))),
            ("bytes" | "byte", Some(number)) => Some(Value::Bytes(number)),
            ("seconds" | "second", Some(number)) => Some(Value::Seconds(number as u64)),
            _ => None,
        };
        if let Some(value) = value {
            out.push((start, format!("{raw_number} {unit}"), value));
        }
        index = unit_end.max(index);
    }
    out
}

/// The documented limit a stated quantity is attributed to: the key that names a
/// limit in the quantity's own clause. A value repeated in prose beside a
/// `` `key = value` `` pair is then compared with that key rather than merely being
/// required to appear somewhere among the constants, so
/// ``The aggregate input limit (`total-bytes = 64 MiB`) stops at 8 MiB`` fails: both
/// quantities are in the constant set, but `8 MiB` belongs to `source-bytes`.
///
/// The clause scope matters on its own: ``(`total-bytes = 64 MiB`) stops at 8 MiB;
/// `source-bytes` is per file`` names the right key on the same line, and a nearest
/// key picked across the whole line handed that aggregate quantity to
/// `source-bytes`, which accepted the false statement.
fn attributed_key<'a>(line: &'a str, offset: usize, keys: &[&'a str]) -> Option<&'a str> {
    let (clause_start, clause) = clause_around(line, offset);
    keys.iter()
        .copied()
        .filter(|key| clause.contains(*key))
        .min_by_key(|key| {
            clause
                .match_indices(*key)
                .map(|(at, _)| at.abs_diff(offset - clause_start))
                .min()
                .unwrap_or(usize::MAX)
        })
}

/// The clause of `line` that contains the byte at `offset`, and where it starts.
/// Boundaries are the ones the vault claim reader uses: a sentence end, a list
/// separator, or a comma followed by whitespace, so the attribution cannot jump
/// over a `;` to a key that belongs to the next statement.
fn clause_around(line: &str, offset: usize) -> (usize, &str) {
    let mut start = 0;
    let mut end = line.len();
    for (index, ch) in line.char_indices() {
        let rest = &line[index + ch.len_utf8()..];
        let next_is_whitespace = rest.chars().next().is_none_or(char::is_whitespace);
        let boundary = match ch {
            '。' => true,
            '、' | '・' | ';' => true,
            ',' => next_is_whitespace || rest.is_empty(),
            '.' => next_is_whitespace,
            _ => false,
        };
        if !boundary {
            continue;
        }
        if index + ch.len_utf8() <= offset {
            start = index + ch.len_utf8();
        } else {
            end = index;
            break;
        }
    }
    (start, &line[start..end])
}

/// Check every stated quantity in one document line against the limit it names.
fn check_stated_line(name: &str, line: &str) -> Result<(), String> {
    for (char_offset, raw, value) in stated_sizes(line) {
        let Some(offset) = line.char_indices().nth(char_offset).map(|(byte, _)| byte) else {
            continue;
        };
        let Some(key) = attributed_key(line, offset, LIMIT_KEYS) else {
            return Err(format!(
                "{name} states {raw} in {line:?} without naming, in the same clause, which limit \
                 it is; state the limit beside the number as `` `key = value` ``, or drop the \
                 number, because a quantity attributed to another statement cannot be checked \
                 against src/documented_limits.rs"
            ));
        };
        let expected = constant(key).unwrap_or_else(|| panic!("{key} is not a documented limit"));
        if value != expected {
            return Err(format!(
                "{name} states {raw} next to `{key}`, but src/documented_limits.rs holds \
                 {expected} for `{key}`; the number must equal the limit it names, not merely \
                 be one of the documented values"
            ));
        }
    }
    Ok(())
}

/// The documents that carry the limit contract may not state a size or duration
/// without checking it against the limit it names. Two shapes of unchecked copy used
/// to survive here: `docs/cli.md` restated the aggregate input limit in a sentence,
/// and the coverage table named its rows by value, so changing only that copy left
/// every drift test green. Matching a value against the whole constant set was not
/// enough either — a `8 MiB` copy beside a `total-bytes = 64 MiB` pair passed — so a
/// quantity is now compared with the specific limit it sits next to.
#[test]
fn every_stated_size_matches_the_limit_it_names() {
    for name in [
        "docs/cli.md",
        "docs/parser-design.md",
        "docs/resource-limits.md",
    ] {
        let text = read(name);
        for line in text.lines() {
            if let Err(error) = check_stated_line(name, line) {
                panic!("{error}");
            }
        }
    }
}

/// The quantities this guard now rejects, pinned: a drifted sentence, a value
/// borrowed from another limit, a `9-second` duration and an unattributed number
/// must all fail, while the correct pair form and a matching restatement pass.
#[test]
fn a_prose_limit_value_beside_a_checked_pair_fails() {
    let good = "The aggregate input limit (`total-bytes = 64 MiB`) stops analysis.\n";
    check_stated_line("synthetic.md", good).expect("the checked pair alone must pass");
    // A prose restatement that matches its own limit is checked, not rejected.
    let restated = "| `source-bytes` | 8 MiB per source | `tests/resource-limits.rs` |\n";
    check_stated_line("synthetic.md", restated)
        .expect("an 8 MiB restatement of `source-bytes` must pass");
    let drifted = "The aggregate input limit (`total-bytes = 64 MiB`) stops at 65 MiB.\n";
    let error = check_stated_line("synthetic.md", drifted)
        .expect_err("a drifted `65 MiB` copy must not pass beside the pair");
    assert!(error.contains("65 MiB"), "{error}");
    assert!(error.contains("`total-bytes`"), "{error}");
    // The cross-limit case: both quantities are documented values, but `8 MiB`
    // belongs to another limit, so a byte-set membership check passed it.
    let cross = "The aggregate input limit (`total-bytes = 64 MiB`) stops at 8 MiB.\n";
    let error = check_stated_line("synthetic.md", cross)
        .expect_err("an `8 MiB` copy must be compared with `total-bytes`, not accepted as a limit");
    assert!(error.contains("`total-bytes`"), "{error}");
    // A second key later in the same line must not take the aggregate quantity.
    let cross_with_key = "The aggregate limit (`total-bytes = 64 MiB`) stops at 8 MiB; `source-bytes` is per file.\n";
    let error = check_stated_line("synthetic.md", cross_with_key)
        .expect_err("an `8 MiB` aggregate claim must not be attributed to `source-bytes`");
    assert!(error.contains("`total-bytes`"), "{error}");
    // The hyphenated duration form the measurement rows use.
    let row = "| `parse-budget` | 2-second parse budget | `src/syntax.rs` |\n";
    check_stated_line("synthetic.md", row)
        .expect("the documented `2-second` duration must pass beside `parse-budget`");
    let drifted_row = "| `parse-budget` | 9-second parse budget | `src/syntax.rs` |\n";
    let error = check_stated_line("synthetic.md", drifted_row)
        .expect_err("a `9-second` duration must be read and rejected, not skipped");
    assert!(error.contains("9 second"), "{error}");
    assert!(error.contains("`parse-budget`"), "{error}");
    // A size with no limit name in its clause cannot be checked at all.
    let orphan = "A 9 MiB source limit applies.\n";
    let error = check_stated_line("synthetic.md", orphan)
        .expect_err("an unattributed `9 MiB` must not pass without naming its limit");
    assert!(
        error.contains("without naming, in the same clause"),
        "{error}"
    );
}

#[test]
fn every_documented_limit_matches_its_constant() {
    let required: &[(&str, &[&str])] = &[
        ("docs/cli.md", INPUT_KEYS),
        ("docs/parser-design.md", PARSER_KEYS),
        ("docs/resource-limits.md", LIMIT_KEYS),
    ];
    for (name, keys) in required {
        let text = read(name);
        for key in keys.iter() {
            let expected = constant(key).unwrap();
            let raw = documented_value(&text, key).unwrap_or_else(|| {
                panic!(
                    "{name} must state the limit as `` `{key} = <value>` ``; \
                     src/documented_limits.rs holds {expected}"
                )
            });
            compare_stated(name, key, &raw);
        }
    }
    // Any other document that states a given key is held to the same value.
    for name in documents() {
        if required
            .iter()
            .any(|(required_name, _)| *required_name == name)
        {
            continue;
        }
        let text = read(&name);
        for key in LIMIT_KEYS {
            if let Some(raw) = documented_value(&text, key) {
                compare_stated(&name, key, &raw);
            }
        }
    }
}

/// The shared toolchain contract is human-owned prose in the maintainer vault and
/// is not part of either repository, so CI cannot read it. Point
/// `VERSE_FMT_CONTRACT_DOC` at that file to check it too: every canonical value
/// must appear in the vault's own claim for that limit, and a changed or missing
/// value fails with its name.
#[test]
fn the_optional_vault_contract_states_the_same_values() {
    let Ok(path) = std::env::var("VERSE_FMT_CONTRACT_DOC") else {
        return;
    };
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read VERSE_FMT_CONTRACT_DOC {path}: {error}"));
    if let Err(error) = check_vault_contract(&path, &text) {
        panic!("{error}");
    }
}

/// The wording the human-owned vault uses to name each limit it restates. The
/// vault is prose, not a key/value table, so a value only counts when it sits in a
/// clause attributed to its own limit: a different limit's value must never
/// satisfy this key's claim. `nesting 257` next to `config-bytes = 256 KiB` is
/// exactly the case the whole-document search used to miss. A reworded vault fails
/// with the key and the wording this guard looked for.
fn vault_namings(key: &str) -> &'static [&'static str] {
    match key {
        "source-bytes" => &["source-bytes", "1ファイル", "per file", "file size"],
        "total-bytes" => &["total-bytes", "全入力", "total input"],
        "nesting" => &["nesting"],
        "sources" => &["sources", "source", "invocation"],
        "visited-entries" => &["visited-entries", "visited", "entry", "訪問"],
        "directory-depth" => &["directory-depth", "directory depth", "depth"],
        "config-bytes" => &["config-bytes", "verse.toml", ".gitignore", "config"],
        "exclude-globs" => &["exclude-globs", "exclude glob", "globs"],
        "exclude-glob-bytes" => &["exclude-glob-bytes", "各", "each", "per glob", "glob bytes"],
        _ => &[],
    }
}

fn clause_names_limit(clause: &str, key: &str) -> bool {
    let clause = clause.to_lowercase();
    vault_namings(key).iter().any(|naming| {
        let naming = naming.to_lowercase();
        clause.contains(naming.as_str())
    })
}

/// Split prose into claim-sized pieces at sentence, list-item and line
/// boundaries. `` `verse.toml` `` keeps its dot: a `.` only ends a sentence when
/// whitespace or the end of the text follows it. Each piece also reports whether
/// it ended a sentence, so attribution cannot leak across a sentence break.
///
/// `,` is a boundary too when whitespace follows it, so `128 KiB, 256 KiB` is two
/// pieces; a thousands separator (`10,000`) is not followed by whitespace and
/// stays inside one piece.
fn clauses(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let rest = &text[index + ch.len_utf8()..];
        let next_is_whitespace = rest.chars().next().is_none_or(char::is_whitespace);
        let (boundary, ends_sentence) = match ch {
            '\n' | '\r' => (true, false),
            '。' => (true, true),
            '、' | '・' | ';' => (true, false),
            ',' => (next_is_whitespace || rest.is_empty(), false),
            '.' => (next_is_whitespace, true),
            _ => (false, false),
        };
        if boundary {
            out.push((&text[start..index], ends_sentence));
            start = index + ch.len_utf8();
        }
    }
    out.push((&text[start..], true));
    out
}

/// Every clause attributed to the limits it names.
///
/// A claim runs from the clause that names a limit to the first clause that
/// carries a quantity — the vault writes upper bounds as `上限は1ファイル
/// 8 MiB`, so a number written after its naming belongs to that limit. Past that
/// number the claim is complete and stops absorbing text, and a sentence break
/// ends it as well. Both rules exist because the previous "keep the last
/// attribution until another limit is named" version passed a `128 KiB`
/// configuration claim: an unrelated `256 KiB` later in the same sentence still
/// satisfied the check, and the claim it printed was the rest of the document.
fn vault_claims(text: &str, keys: &[&str]) -> BTreeMap<String, String> {
    let mut claims: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_holds_number = false;
    for (clause, ends_sentence) in clauses(text) {
        let named: Vec<String> = keys
            .iter()
            .filter(|key| clause_names_limit(clause, key))
            .map(|key| (*key).to_owned())
            .collect();
        let numbers = !quantities(clause).is_empty();
        if !named.is_empty() {
            current = named;
            current_holds_number = numbers;
        } else if numbers {
            // A number in a clause that names no limit is either this claim's
            // value or, when this claim already holds one, the start of something
            // else: a later limit's value must not be absorbed here.
            if current_holds_number {
                current.clear();
                continue;
            }
            current_holds_number = true;
        } else if current_holds_number {
            // The claim is complete; prose after it is not part of the claim.
            current.clear();
            continue;
        }
        for key in &current {
            let claim = claims.entry(key.clone()).or_default();
            claim.push_str(clause);
            claim.push('\n');
        }
        if ends_sentence {
            current.clear();
            current_holds_number = false;
        }
    }
    claims
}

fn check_vault_contract(path: &str, text: &str) -> Result<(), String> {
    let claims = vault_claims(text, INPUT_KEYS);
    for key in INPUT_KEYS {
        let expected = constant(key).unwrap();
        let Some(claim) = claims.get(*key) else {
            return Err(format!(
                "{path} never names the `{key}` limit; this guard looks for {:?}. State the \
                 limit's name and value in one claim, or teach \
                 tests/docs-drift.rs::vault_namings the new wording",
                vault_namings(key)
            ));
        };
        let found = quantities(claim);
        if !found.contains(&expected) {
            return Err(format!(
                "{path} states `{key}` as {claim:?}, which holds {found:?} instead of \
                 {expected} (acceptable wording: {:?}); src/documented_limits.rs holds \
                 {expected}",
                renderings(expected)
            ));
        }
    }
    Ok(())
}

/// Every numeric quantity in a claim, read as a value and its unit. A claim
/// states a limit only when one of these equals the constant exactly, so `1256`
/// cannot satisfy 256 and `128 MiB` cannot satisfy 8 MiB, the two ways a plain
/// substring search still passed a wrong document. An unknown unit — `256`
/// followed by a word this guard does not know — is dropped rather than guessed,
/// because guessing a unit is how a wrong value slips through.
fn quantities(text: &str) -> Vec<Value> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == ',') {
            index += 1;
        }
        let digits: String = chars[start..index]
            .iter()
            .filter(|ch| ch.is_ascii_digit())
            .collect();
        let mut after = index;
        while after < chars.len() && chars[after].is_whitespace() {
            after += 1;
        }
        let mut unit_end = after;
        while unit_end < chars.len() && chars[unit_end].is_ascii_alphanumeric() {
            unit_end += 1;
        }
        let unit: String = chars[after..unit_end].iter().collect();
        let Ok(number) = digits.parse::<usize>() else {
            continue;
        };
        let value = match unit.as_str() {
            "MiB" => Some(Value::Bytes(number.saturating_mul(MIB))),
            "KiB" => Some(Value::Bytes(number.saturating_mul(KIB))),
            "bytes" | "byte" => Some(Value::Bytes(number)),
            "seconds" | "second" => Some(Value::Seconds(number as u64)),
            _ if unit.is_empty() => match chars.get(after) {
                // A Japanese counter or a line/sentence boundary ends the quantity;
                // an ASCII letter would start an unknown unit, which is dropped.
                Some('秒') => Some(Value::Seconds(number as u64)),
                Some(ch) if !ch.is_ascii_alphanumeric() => Some(Value::Count(number)),
                Some(_) => None,
                None => Some(Value::Count(number)),
            },
            _ => None,
        };
        if let Some(value) = value {
            out.push(value);
        }
        index = unit_end.max(index);
    }
    out
}

/// The vault is not in this repository, so its live check cannot run in CI: the
/// attribution and the quantity comparison are pinned here. A value belonging to
/// another limit must not satisfy a claim, a larger number must not satisfy a
/// smaller one, and the documented values in the same document must still pass.
#[test]
fn a_vault_value_cannot_satisfy_another_limits_claim() {
    let headline = |nesting: u32| {
        format!(
            "構文nesting {nesting}。上限は1ファイル8 MiB。全入力64 MiB。\
             1 invocationあたりsource 10,000件。訪問entry 100,000件。\
             directory depth 128。`verse.toml` / 各`.gitignore` 256 KiB。\
             exclude glob 256件・各4,096 bytes。"
        )
    };
    let error = check_vault_contract("synthetic-contract.md", &headline(257))
        .expect_err("nesting 257 must not pass through the `256 KiB` config-bytes claim");
    assert!(error.contains("`nesting`"), "{error}");
    check_vault_contract("synthetic-contract.md", &headline(256))
        .expect("the documented nesting value must pass in the same document");
    // The attribution also holds in the other direction: a wrong value in a claim
    // whose name still matches must fail even when another claim holds the right
    // number, so the config size cannot hide behind the glob count's `256`.
    let misplaced = headline(256).replace(
        "`verse.toml` / 各`.gitignore` 256 KiB",
        "`verse.toml` / 各`.gitignore` 128 KiB",
    );
    assert_ne!(misplaced, headline(256), "the fixture wording moved");
    let error = check_vault_contract("synthetic-contract.md", &misplaced)
        .expect_err("a 128 KiB config claim must not pass because the glob count reads 256");
    assert!(error.contains("`config-bytes`"), "{error}");
    // A quantity is compared, not matched as a substring: `1256` must not satisfy
    // 256, and a 128 MiB source must not satisfy 8 MiB.
    let error = check_vault_contract("synthetic-contract.md", &headline(1256))
        .expect_err("nesting 1256 contains `256` but is not 256 levels");
    assert!(error.contains("`nesting`"), "{error}");
    let oversized = headline(256).replace("1ファイル8 MiB", "1ファイル128 MiB");
    assert_ne!(oversized, headline(256), "the fixture wording moved");
    let error = check_vault_contract("synthetic-contract.md", &oversized)
        .expect_err("a `128 MiB` source limit contains `8 MiB` but is not 8 MiB");
    assert!(error.contains("`source-bytes`"), "{error}");
}

/// The exact shape that used to pass: the configuration claim says `128 KiB`, and
/// the same sentence carries an unrelated clause with `256 KiB`. Attribution must
/// stop at the first number a claim holds, so the unrelated `256 KiB` cannot
/// satisfy a claim that states 128 KiB, and the reported claim is the limit's own
/// sentence, not the rest of the document.
#[test]
fn an_unrelated_later_number_cannot_satisfy_a_wrong_claim() {
    let text = "構文nesting 256。上限は1ファイル8 MiB。全入力64 MiB。\
1 invocationあたりsource 10,000件。訪問entry 100,000件。\
directory depth 128。\
`verse.toml` / 各`.gitignore` 128 KiB, 計測用の固定入力は256 KiB。\
exclude glob 256件・各4,096 bytes。\n";
    let error = check_vault_contract("synthetic-contract.md", text)
        .expect_err("a 128 KiB config claim must not pass because a later clause reads 256 KiB");
    assert!(error.contains("`config-bytes`"), "{error}");
    // The claim that failed is the limit's own text: the unrelated clause is not
    // absorbed, and neither is the rest of the document.
    assert!(error.contains("128 KiB"), "{error}");
    assert!(!error.contains("exclude glob"), "{error}");
    // The same document with the real 256 KiB value still passes, so the rule
    // rejects the wrong number rather than every sentence with two quantities.
    let fixed = text.replace("`.gitignore` 128 KiB", "`.gitignore` 256 KiB");
    assert_ne!(fixed, text, "the fixture wording moved");
    check_vault_contract("synthetic-contract.md", &fixed)
        .expect("the documented config value must pass in the same sentence");
}

/// Every way a document may reasonably write this value.
fn renderings(value: Value) -> Vec<String> {
    match value {
        Value::Bytes(bytes) => {
            let mut out = vec![format!("{} bytes", grouped(bytes))];
            if bytes % MIB == 0 {
                out.push(format!("{} MiB", bytes / MIB));
            }
            if bytes % KIB == 0 {
                out.push(format!("{} KiB", bytes / KIB));
            }
            out
        }
        Value::Count(count) => vec![count.to_string(), grouped(count)],
        Value::Seconds(seconds) => {
            vec![format!("{seconds} seconds"), format!("{seconds}-second")]
        }
    }
}

fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

// ------------------------------------------------------------ grammar coverage

#[derive(Clone, Copy, Debug)]
enum Handling {
    Supported,
    Refused,
}

impl Handling {
    fn exit_code(self) -> i32 {
        match self {
            Self::Supported => 0,
            Self::Refused => 2,
        }
    }
}

impl fmt::Display for Handling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Supported => write!(f, "supported"),
            Self::Refused => write!(f, "refused (exit 2)"),
        }
    }
}

/// The columns of the coverage table, in order. `Form`, `Handling` and `Fixture`
/// are executed against the binary; the others are checked for presence and shape
/// so no cell can be left silently blank.
const COVERAGE_HEADER: [&str; 9] = [
    "Form",
    "Handling",
    "Reference",
    "Test",
    "Lossless",
    "Formatted",
    "Diagnostic",
    "UEFN build",
    "Fixture",
];

struct Row {
    form: String,
    handling: Handling,
    fixtures: Vec<String>,
    test: String,
    diagnostic: String,
    uefn_build: String,
}

fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Parse the checkable part of `docs/grammar-coverage.md`: a table whose columns
/// are [`COVERAGE_HEADER`], with one or more fixture IDs per row. Every row must
/// classify itself as `supported` or `refused`, and no cell may be blank.
fn coverage_rows() -> Vec<Row> {
    let text = read("docs/grammar-coverage.md");
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        if !line.trim_start().starts_with('|') {
            if in_table {
                break;
            }
            continue;
        }
        let columns = cells(line);
        if columns == COVERAGE_HEADER {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if columns
            .iter()
            .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }
        assert_eq!(
            columns.len(),
            COVERAGE_HEADER.len(),
            "docs/grammar-coverage.md row {line:?} must have the columns {COVERAGE_HEADER:?}"
        );
        for (name, cell) in COVERAGE_HEADER.iter().zip(&columns) {
            assert!(
                !cell.is_empty(),
                "docs/grammar-coverage.md row {:?} leaves the `{name}` column blank; \
                 state the value, `none recorded`, `no test` or `not verified: <reason>`",
                columns[0]
            );
        }
        let handling = if columns[1].starts_with("supported") {
            Handling::Supported
        } else if columns[1].starts_with("refused") {
            Handling::Refused
        } else {
            panic!(
                "docs/grammar-coverage.md row {:?} has handling {:?}; it must start with \
                 `supported` or `refused` so this guard can check it",
                columns[0], columns[1]
            )
        };
        let fixtures = backticked(columns[8]);
        assert!(
            !fixtures.is_empty(),
            "docs/grammar-coverage.md row {:?} names no fixture; add its ID from \
             tests/fixtures/coverage.json so the claim is executable",
            columns[0]
        );
        rows.push(Row {
            form: columns[0].to_owned(),
            handling,
            fixtures,
            test: columns[3].to_owned(),
            diagnostic: columns[6].to_owned(),
            uefn_build: columns[7].to_owned(),
        });
    }
    assert!(
        in_table,
        "docs/grammar-coverage.md has no `| {} |` table; the guard cannot check its claims",
        COVERAGE_HEADER.join(" | ")
    );
    rows
}

#[derive(serde::Deserialize)]
struct Coverage {
    cases: Vec<CoverageCase>,
}

#[derive(serde::Deserialize)]
struct CoverageCase {
    id: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    bytes: Option<Vec<u8>>,
    #[serde(default)]
    generated: Option<String>,
}

impl CoverageCase {
    fn content(&self) -> Vec<u8> {
        match (&self.source, &self.bytes, &self.generated) {
            (Some(source), None, None) => source.as_bytes().to_vec(),
            (None, Some(bytes), None) => bytes.clone(),
            (None, None, Some(generated)) => generated_fixture(generated),
            _ => panic!(
                "tests/fixtures/coverage.json case {:?} must set exactly one of source, bytes or generated",
                self.id
            ),
        }
    }
}

fn generated_fixture(name: &str) -> Vec<u8> {
    match name {
        "indentation-depth-251" => deep_indentation(251).into_bytes(),
        "indentation-depth-252" => deep_indentation(252).into_bytes(),
        other => panic!(
            "tests/fixtures/coverage.json names generated fixture {other:?}, \
             which this test cannot build"
        ),
    }
}

fn deep_indentation(levels: usize) -> String {
    let mut source = String::from("F():void =\n");
    for level in 1..=levels {
        source.push_str(&"    ".repeat(level));
        source.push_str("if (true):\n");
    }
    source.push_str(&"    ".repeat(levels + 1));
    source.push_str("Print(\"deep\")\n");
    source
}

fn run(root: &Path, bytes: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn every_grammar_coverage_row_matches_the_pinned_build() {
    let coverage: Coverage = serde_json::from_str(&read("tests/fixtures/coverage.json"))
        .expect("tests/fixtures/coverage.json must stay parseable");
    let fixtures: BTreeMap<&str, &CoverageCase> = coverage
        .cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    let rows = coverage_rows();
    assert!(
        rows.len() >= 20,
        "docs/grammar-coverage.md lost rows: only {} are checkable",
        rows.len()
    );
    let dir = tempfile::tempdir().unwrap();
    let mut used = BTreeSet::new();
    for row in &rows {
        for id in &row.fixtures {
            let Some(case) = fixtures.get(id.as_str()) else {
                panic!(
                    "docs/grammar-coverage.md row {:?} names fixture {id:?}, which \
                     tests/fixtures/coverage.json does not define",
                    row.form
                );
            };
            let out = run(dir.path(), &case.content());
            assert_eq!(
                out.status.code(),
                Some(row.handling.exit_code()),
                "docs/grammar-coverage.md marks {:?} as `{}`, but fixture {id:?} exited {:?}: {}",
                row.form,
                row.handling,
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
            used.insert(case.id.clone());
        }
    }
    let unused: Vec<&str> = coverage
        .cases
        .iter()
        .filter(|case| !used.contains(&case.id))
        .map(|case| case.id.as_str())
        .collect();
    assert!(
        unused.is_empty(),
        "tests/fixtures/coverage.json defines fixtures no grammar-coverage row uses: {unused:?}"
    );
}

/// The reference build the UEFN-build column is to be verified against.
const UEFN_REFERENCE_BUILD: &str = "++Fortnite+Release-42.20-CL-58011042-Windows";

/// Whether `fn name` is defined in the file at `path` (relative to the repository).
fn defines_test(path: &str, name: &str) -> bool {
    let file = root().join(path);
    fs::read_to_string(file).is_ok_and(|text| text.contains(&format!("fn {name}(")))
}

#[test]
fn every_row_cites_real_tests_or_says_no_test() {
    let corpus: serde_json::Value = serde_json::from_str(&read("tests/fixtures/corpus.json"))
        .expect("tests/fixtures/corpus.json must stay parseable");
    let corpus_ids: BTreeSet<&str> = corpus["cases"]
        .as_array()
        .expect("corpus cases")
        .iter()
        .filter_map(|case| case["id"].as_str())
        .collect();
    for row in coverage_rows() {
        let cited = backticked(&row.test);
        if row.test.starts_with("no test") {
            assert!(
                cited.is_empty(),
                "docs/grammar-coverage.md row {:?} says `no test` but also cites {cited:?}",
                row.form
            );
            assert!(
                matches!(row.handling, Handling::Supported),
                "docs/grammar-coverage.md row {:?} is refused, so a test must cite the refusal",
                row.form
            );
            continue;
        }
        assert!(
            !cited.is_empty(),
            "docs/grammar-coverage.md row {:?} has a Test cell {:?} that names no test; \
             cite one or write `no test`",
            row.form,
            row.test
        );
        for reference in cited {
            match reference.split_once("::") {
                Some((path, name)) => assert!(
                    defines_test(path, name),
                    "docs/grammar-coverage.md row {:?} cites {reference:?}, but `fn {name}` \
                     is not defined in {path}",
                    row.form
                ),
                None => assert!(
                    corpus_ids.contains(reference.as_str()),
                    "docs/grammar-coverage.md row {:?} cites corpus case {reference:?}, \
                     which tests/fixtures/corpus.json does not define",
                    row.form
                ),
            }
        }
    }
}

#[test]
fn every_refused_row_names_the_guard_that_refuses_it() {
    for row in coverage_rows() {
        if matches!(row.handling, Handling::Refused) {
            assert!(
                row.diagnostic.contains("`src/") && row.diagnostic.contains("[stage="),
                "docs/grammar-coverage.md row {:?} is refused but its Diagnostic cell {:?} \
                 names neither the message stage nor the `src/` code path that refuses it",
                row.form,
                row.diagnostic
            );
        }
    }
}

#[test]
fn the_grammar_table_records_the_unverified_uefn_column() {
    for row in coverage_rows() {
        assert!(
            row.uefn_build.starts_with("not verified: ") && row.uefn_build.len() > 20,
            "docs/grammar-coverage.md row {:?} has UEFN build {:?}; until a human-run build \
             records a result it must read `not verified: <reason>`, and no row may claim \
             UEFN acceptance",
            row.form,
            row.uefn_build
        );
    }
    let text = read("docs/grammar-coverage.md");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("UEFN-build column is unverified"),
        "docs/grammar-coverage.md must state plainly that the UEFN-build column is \
         unverified, instead of leaving the gap implied"
    );
    assert!(
        flat.contains(UEFN_REFERENCE_BUILD) && flat.contains("not yet verified"),
        "docs/grammar-coverage.md must record {UEFN_REFERENCE_BUILD} as the build to verify \
         against and mark it not yet verified"
    );
    for heading in ["Agent-verifiable columns", "Human-gated column"] {
        assert!(
            text.contains(heading),
            "docs/grammar-coverage.md preamble must say which columns are agent-verifiable \
             and which need a human-run UEFN build: missing {heading:?}"
        );
    }
}

// ------------------------------------------------------------------ README gate

/// The README is where a reader asks "does this handle my code?", so the
/// limitation and release-gate sections it carries are pinned: a rewrite that drops
/// one, loses the link to the coverage table or the sibling tool, or starts to
/// claim UEFN acceptance fails here instead of drifting silently. Numbers in those
/// sections stay covered by the corpus and limit guards above.
#[test]
fn the_readme_states_known_limitations_and_the_release_gate() {
    let text = read("README.md");
    for heading in ["## Known limitations", "## Release gate", "## Related tool"] {
        assert!(
            text.lines().any(|line| line == heading),
            "README.md lost its `{heading}` section"
        );
    }
    let section = |heading: &str| -> String {
        text.split(heading)
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .unwrap_or_else(|| panic!("README.md must carry a `{heading}` section"))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let limitations = section("## Known limitations");
    let gate = section("## Release gate");
    for (needle, why) in [
        (
            "No UEFN compiler acceptance has been performed",
            "UEFN acceptance",
        ),
        ("Unreleased alpha", "alpha status"),
        ("docs/grammar-coverage.md", "the coverage table"),
        (
            "Clean-machine operation is unverified",
            "clean-machine status",
        ),
        (
            "mise installation is documented but untested",
            "mise status",
        ),
        ("no git tag", "the absent tag"),
    ] {
        assert!(
            limitations.contains(needle),
            "README.md Known limitations must state {why} (`{needle}`)"
        );
    }
    for needle in ["Verification gates", "Human-owned gates"] {
        assert!(
            gate.contains(needle),
            "README.md Release gate must separate gates; `{needle}` is missing"
        );
    }
    assert!(
        text.contains("https://github.com/eiei114/verse-lint") && text.contains("independent"),
        "README.md must link the sibling linter and state that the tools are independent"
    );
    let lowered = text.to_lowercase();
    for claim in [
        "accepted by uefn",
        "uefn acceptance is complete",
        "uefn acceptance passed",
        "production ready",
        "production-ready",
        "clean-machine verified",
    ] {
        assert!(
            !lowered.contains(claim),
            "README.md must not claim `{claim}`; no UEFN acceptance has been performed"
        );
    }
}
