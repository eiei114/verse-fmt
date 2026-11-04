use std::fmt;

pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NESTING: usize = 256;

/// Failure stage vocabulary. Each variant names a pipeline phase this binary
/// can actually distinguish; the names are the wire values of
/// `reason.stage` in the JSON report (`docs/report-contract.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Input discovery, path guards and configuration resolution. Reserved by
    /// the shared report contract; no refusal emitted today carries it.
    #[allow(dead_code)]
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
    /// Write preflight and commit. Reserved: a write failure is reported as an
    /// execution error, never as a refused file (`src/main.rs::write_failure`).
    /// No `files` entry carries this stage today.
    #[allow(dead_code)]
    Write,
    /// Emitting human or machine output. Reserved by the shared report contract.
    #[allow(dead_code)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub stage: Stage,
    pub offset: usize,
    pub message: String,
}

impl Failure {
    pub fn new(stage: Stage, offset: usize, message: impl Into<String>) -> Self {
        Self {
            stage,
            offset,
            message: message.into(),
        }
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
