use std::fmt;

pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NESTING: usize = 256;

/// Failure stage vocabulary. Each variant names a pipeline phase this binary
/// can actually distinguish; the names are the wire values of
/// `reason.stage` in the JSON report (`docs/report-contract.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Input discovery, path guards, configuration resolution and path
    /// labelling. A discovery failure happens before a source exists, so it is
    /// an `executionErrors` entry, never a `refused` file.
    Discovery,
    /// Reading and decoding a source (size, UTF-8, NUL, bare CR).
    Source,
    Lexer,
    Parser,
    /// Indentation and block-structure analysis.
    Structure,
    /// Layout emission.
    Layout,
    /// Post-format preservation and idempotence checks.
    Validation,
    /// Write preflight, staging and commit. A write failure is an execution
    /// error for one path, never a `refused` file (`src/main.rs::write_failure`).
    Write,
    /// Emitting human or machine output.
    Reporting,
}

impl Stage {
    /// Wire names in schema order, used to keep `docs/diagnostics.schema.json`
    /// and this vocabulary in lockstep.
    #[cfg(test)]
    pub const ALL: [Self; 9] = [
        Self::Discovery,
        Self::Source,
        Self::Lexer,
        Self::Parser,
        Self::Structure,
        Self::Layout,
        Self::Validation,
        Self::Write,
        Self::Reporting,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Source => "source",
            Self::Lexer => "lexer",
            Self::Parser => "parser",
            Self::Structure => "structure",
            Self::Layout => "layout",
            Self::Validation => "validation",
            Self::Write => "write",
            Self::Reporting => "reporting",
        }
    }
}

impl serde::Serialize for Stage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// Origin of a failure, for the one stage that has two distinguishable ones.
///
/// The adjacent-structure guard and a raw CST `ERROR`/`MISSING` finding both
/// stop processing in `parser`, and a reader must be able to tell a guard
/// refusal from a parse-error refusal. Every other stage has a single origin
/// and carries no class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// The adjacent-structure guard refused the source before it was trusted.
    Guard,
    /// The bounded CST walk found a raw `ERROR` or `MISSING` node.
    Parse,
}

impl Class {
    /// Wire names in schema order, used to keep `docs/diagnostics.schema.json`
    /// and this vocabulary in lockstep.
    #[cfg(test)]
    pub const ALL: [Self; 2] = [Self::Guard, Self::Parse];

    pub fn name(self) -> &'static str {
        match self {
            Self::Guard => "guard",
            Self::Parse => "parse",
        }
    }
}

impl serde::Serialize for Class {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// A failure recorded after the primary one stopped processing. The adjacent
/// guard is primary; a raw parse error underneath it is secondary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Underlying {
    pub stage: Stage,
    pub class: Class,
    pub offset: usize,
    /// Tree-sitter node kind: `ERROR`, or `MISSING <kind>` for a zero-width
    /// node the grammar had to insert.
    pub node_kind: String,
    /// The underlying diagnostic naming itself as the underlying cause.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub stage: Stage,
    pub offset: usize,
    pub message: String,
    /// Set only where one stage has two distinguishable origins.
    pub class: Option<Class>,
    /// Failures recorded after this one stopped processing, in discovery order.
    pub underlying: Vec<Underlying>,
}

impl Failure {
    pub fn new(stage: Stage, offset: usize, message: impl Into<String>) -> Self {
        Self {
            stage,
            offset,
            message: message.into(),
            class: None,
            underlying: Vec::new(),
        }
    }

    /// The adjacent-structure guard refused the source. The guard stays the
    /// primary refusal; an underlying parse error is attached later when the
    /// same bounded CSS walk finds one.
    pub fn guarded(offset: usize, message: impl Into<String>) -> Self {
        Self {
            stage: Stage::Parser,
            offset,
            message: message.into(),
            class: Some(Class::Guard),
            underlying: Vec::new(),
        }
    }

    /// A raw CST `ERROR`/`MISSING` node refused the source.
    pub fn parse(offset: usize, message: impl Into<String>) -> Self {
        Self {
            stage: Stage::Parser,
            offset,
            message: message.into(),
            class: Some(Class::Parse),
            underlying: Vec::new(),
        }
    }

    pub fn with_underlying(mut self, underlying: Underlying) -> Self {
        self.underlying.push(underlying);
        self
    }
}

/// Stages one source actually entered, in execution order.
///
/// The pipeline is sequential, but `validation` re-enters `lexer`, `parser` and
/// `structure` to reparse the output. Every entry is recorded, so the trace is
/// the order that really ran and its last entry is the stage that stopped
/// processing. `--explain` prints it; the JSON report does not carry it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Trace {
    stages: Vec<Stage>,
}

impl Trace {
    pub fn enter(&mut self, stage: Stage) {
        self.stages.push(stage);
    }

    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (byte {})", self.message, self.offset)
    }
}

#[derive(Debug)]
pub struct Source {
    text: String,
    lines: Vec<usize>,
    crlf: bool,
}

impl Source {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(Failure::new(
                Stage::Source,
                0,
                "source exceeds the 8 MiB limit",
            ));
        }
        let text = std::str::from_utf8(bytes).map_err(|e| {
            Failure::new(
                Stage::Source,
                e.valid_up_to(),
                "source must be valid UTF-8 (UTF-16 is unsupported)",
            )
        })?;
        if let Some(i) = bytes.iter().position(|b| *b == 0) {
            return Err(Failure::new(
                Stage::Source,
                i,
                "NUL is not supported in Verse source",
            ));
        }
        let mut crlf = false;
        let mut lines = vec![0];
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\r' && bytes.get(i + 1) != Some(&b'\n') {
                return Err(Failure::new(
                    Stage::Source,
                    i,
                    "bare CR line endings are unsupported",
                ));
            }
            if *b == b'\n' {
                crlf = i > 0 && bytes[i - 1] == b'\r';
                lines.push(i + 1);
            }
        }
        Ok(Self {
            text: text.into(),
            lines,
            crlf,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn body_start(&self) -> usize {
        if self.text.starts_with('\u{feff}') {
            3
        } else {
            0
        }
    }

    pub fn newline(&self) -> &'static str {
        if self.crlf { "\r\n" } else { "\n" }
    }

    /// Byte offsets include the BOM; human columns count Unicode scalars without it.
    pub fn position(&self, offset: usize) -> (usize, usize) {
        let offset = self.text.floor_char_boundary(offset.min(self.text.len()));
        let line = self
            .lines
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let start = if line == 0 {
            self.body_start().min(offset)
        } else {
            self.lines[line]
        };
        (line + 1, self.text[start..offset].chars().count() + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_bytes_and_unicode_positions() {
        let text = "\u{feff}A := \"日😀e\u{301}\"\r\nB:=1\r\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        assert_eq!(source.text(), text);
        assert_eq!(source.body_start(), 3);
        assert_eq!(source.newline(), "\r\n");
        assert_eq!(source.position(text.find('日').unwrap()), (1, 7));
        assert_eq!(source.position(text.find('B').unwrap()), (2, 1));
    }

    #[test]
    fn preserves_mixed_line_ending_positions_and_last_style() {
        let source = Source::from_bytes(b"A\r\nB\nC\r\nD\n").unwrap();
        assert_eq!(source.position(3), (2, 1));
        assert_eq!(source.position(5), (3, 1));
        assert_eq!(source.newline(), "\n");
    }

    #[test]
    fn rejects_unrepresentable_inputs() {
        for bytes in [&b"\xff"[..], b"A\0", b"A\rB", b"\xff\xfeA\0"] {
            assert!(Source::from_bytes(bytes).is_err());
        }
        assert!(Source::from_bytes(&vec![b'a'; MAX_SOURCE_BYTES + 1]).is_err());
    }

    #[test]
    fn a_guard_refusal_can_carry_an_underlying_parse_error() {
        let failure = Failure::guarded(22, "ambiguous").with_underlying(Underlying {
            stage: Stage::Parser,
            class: Class::Parse,
            offset: 24,
            node_kind: "ERROR".into(),
            message: "1:25: underlying parse error: ERROR node at byte 24".into(),
        });
        assert_eq!(failure.class, Some(Class::Guard));
        assert_eq!(failure.underlying.len(), 1);
        assert_eq!(failure.underlying[0].class, Class::Parse);
        assert_eq!(failure.to_string(), "ambiguous (byte 22)");
    }

    #[test]
    fn a_trace_records_the_execution_order_including_a_validation_reparse() {
        let mut trace = Trace::default();
        for stage in [
            Stage::Source,
            Stage::Lexer,
            Stage::Parser,
            Stage::Structure,
            Stage::Layout,
            Stage::Validation,
            Stage::Lexer,
            Stage::Parser,
        ] {
            trace.enter(stage);
        }
        assert_eq!(
            trace
                .stages()
                .iter()
                .map(|stage| stage.name())
                .collect::<Vec<_>>()
                .join(" -> "),
            "source -> lexer -> parser -> structure -> layout -> validation -> lexer -> parser"
        );
    }

    #[test]
    fn class_vocabulary_matches_the_committed_schema() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../docs/diagnostics.schema.json")).unwrap();
        let documented: Vec<&str> = schema["$defs"]["class"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|class| class.as_str().unwrap())
            .collect();
        let implemented: Vec<&str> = Class::ALL.iter().map(|class| class.name()).collect();
        assert_eq!(documented, implemented);
    }

    #[test]
    fn stage_vocabulary_matches_the_committed_schema() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../docs/diagnostics.schema.json")).unwrap();
        let documented: Vec<&str> = schema["$defs"]["reason"]["properties"]["stage"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|stage| stage.as_str().unwrap())
            .collect();
        let implemented: Vec<&str> = Stage::ALL.iter().map(|stage| stage.name()).collect();
        assert_eq!(documented, implemented);
    }
}
