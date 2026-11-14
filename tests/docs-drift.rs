//! Documentation drift guard.
//!
//! Four documented surfaces are checked here against the code, fixtures and the
//! sibling repository they describe: the corpus counts, every resource limit, the
//! grammar-coverage table and the shared contract version. A mismatch fails with
//! the document, the claim and the actual value, so the fix is obvious.
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

/// Limits the parser document restates.
const PARSER_KEYS: &[&str] = &["tokens", "parse-budget", "cst-depth", "fingerprint-events"];

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

/// `README.md` plus every Markdown document under `docs/`, so a new document
/// that quotes a number is checked without editing this test.
fn documents() -> Vec<String> {
    let mut names = vec!["README.md".to_owned()];
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

#[derive(Debug, PartialEq, Eq)]
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
        if !quotes_corpus_counts(&text) {
            continue;
        }
        let line = text
            .lines()
            .find(|line| line.contains(CORPUS_CLAIM))
            .unwrap_or_else(|| {
                panic!(
                    "{name} quotes corpus counts but has no `{CORPUS_CLAIM} <total> cases total, \
                 <golden> golden and <refusal> refusal` line; the corpus has {counts}"
                )
            });
        let claimed = Counts {
            total: number_before(line, "cases total").unwrap_or_else(|error| {
                panic!(
                    "{name}: claim line {line:?} is unreadable: {error}; the corpus has {counts}"
                )
            }),
            golden: number_before(line, "golden").unwrap_or_else(|error| {
                panic!(
                    "{name}: claim line {line:?} is unreadable: {error}; the corpus has {counts}"
                )
            }),
            refusal: number_before(line, "refusal").unwrap_or_else(|error| {
                panic!(
                    "{name}: claim line {line:?} is unreadable: {error}; the corpus has {counts}"
                )
            }),
        };
        assert_eq!(
            claimed, counts,
            "{name} claims `{line}`; tests/fixtures/corpus.json actually holds {counts}"
        );
        checked.push(name);
    }
    assert!(
        !checked.is_empty(),
        "no document quotes the corpus counts; README.md must state them explicitly"
    );
}

// -------------------------------------------------------------- resource limits

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// boundaries. `` `verse.toml` `` keeps its dot: a `.` only ends a clause when
/// whitespace or the end of the text follows it.
fn clauses(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let boundary = match ch {
            '\n' | '\r' | '。' | '、' | '・' | ';' => true,
            '.' => text[index + 1..]
                .chars()
                .next()
                .is_none_or(|next| next.is_whitespace()),
            _ => false,
        };
        if boundary {
            out.push(&text[start..index]);
            start = index + ch.len_utf8();
        }
    }
    out.push(&text[start..]);
    out
}

/// Every clause attributed to the limits it names. A clause that names no limit
/// keeps the previous attribution, so a value written after its naming — as the
/// vault's single limit sentence does — still belongs to that limit.
fn vault_claims(text: &str, keys: &[&str]) -> BTreeMap<String, String> {
    let mut claims: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Vec<String> = Vec::new();
    for clause in clauses(text) {
        let named: Vec<String> = keys
            .iter()
            .filter(|key| clause_names_limit(clause, key))
            .map(|key| (*key).to_owned())
            .collect();
        if !named.is_empty() {
            current = named;
        }
        for key in &current {
            let claim = claims.entry(key.clone()).or_default();
            claim.push_str(clause);
            claim.push('\n');
        }
    }
    claims
}

fn check_vault_contract(path: &str, text: &str) -> Result<(), String> {
    let claims = vault_claims(text, INPUT_KEYS);
    for key in INPUT_KEYS {
        let expected = constant(key).unwrap();
        let renderings = renderings(expected);
        let Some(claim) = claims.get(*key) else {
            return Err(format!(
                "{path} never names the `{key}` limit; this guard looks for {:?}. State the \
                 limit's name and value in one claim, or teach \
                 tests/docs-drift.rs::vault_namings the new wording",
                vault_namings(key)
            ));
        };
        if !renderings.iter().any(|rendering| claim.contains(rendering)) {
            return Err(format!(
                "{path} states `{key}` as {claim:?}, which does not contain {expected} in any \
                 of {renderings:?}; src/documented_limits.rs holds {expected}"
            ));
        }
    }
    Ok(())
}

/// The vault is not in this repository, so its live check cannot run in CI: the
/// scoping itself is pinned here. A value belonging to another limit must not
/// satisfy a claim, and the documented value in the same document must still pass.
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
    // The scoping also holds in the other direction: a wrong value in a claim
    // whose name still matches must fail even when another claim holds the right
    // number, so the config size cannot hide behind the glob count's `256`.
    let misplaced = headline(256).replace(
        "`verse.toml` / 各`.gitignore` 256 KiB",
        "`verse.toml` / 各`.gitignore` 128 KiB",
    );
    let error = check_vault_contract("synthetic-contract.md", &misplaced)
        .expect_err("a 128 KiB config claim must not pass because the glob count reads 256");
    assert!(error.contains("`config-bytes`"), "{error}");
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

struct Row {
    form: String,
    handling: Handling,
    fixtures: Vec<String>,
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
/// are `Form | Handling | Evidence | Fixture`, with one or more fixture IDs per
/// row. Every row must classify itself as `supported` or `refused`.
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
        if columns == ["Form", "Handling", "Evidence", "Fixture"] {
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
            4,
            "docs/grammar-coverage.md row {line:?} must have four columns: Form, Handling, Evidence, Fixture"
        );
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
        let fixtures = backticked(columns[3]);
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
        });
    }
    assert!(
        in_table,
        "docs/grammar-coverage.md has no `| Form | Handling | Evidence | Fixture |` table; \
         the guard cannot check its claims"
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
        "indentation-depth-252" => deep_indentation(252).into_bytes(),
        "indentation-depth-253" => deep_indentation(253).into_bytes(),
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

#[test]
fn the_grammar_table_records_the_unverified_uefn_column() {
    let text = read("docs/grammar-coverage.md");
    assert!(
        text.contains("UEFN-build column") && text.to_lowercase().contains("unverified"),
        "docs/grammar-coverage.md must state plainly that the UEFN-build column is \
         unverified and why, instead of leaving the gap implied"
    );
    assert!(
        !text.contains("| UEFN"),
        "docs/grammar-coverage.md must not present an UEFN column until a human-run \
         build records real results; the gap is stated in prose on purpose"
    );
}
