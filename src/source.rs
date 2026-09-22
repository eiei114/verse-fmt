use std::fmt;

pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NESTING: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub offset: usize,
    pub message: String,
}

impl Failure {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
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
            return Err(Failure::new(0, "source exceeds the 8 MiB limit"));
        }
        let text = std::str::from_utf8(bytes).map_err(|e| {
            Failure::new(
                e.valid_up_to(),
                "source must be valid UTF-8 (UTF-16 is unsupported)",
            )
        })?;
        if let Some(i) = bytes.iter().position(|b| *b == 0) {
            return Err(Failure::new(i, "NUL is not supported in Verse source"));
        }
        let (mut lf, mut crlf) = (false, false);
        let mut lines = vec![0];
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\r' && bytes.get(i + 1) != Some(&b'\n') {
                return Err(Failure::new(i, "bare CR line endings are unsupported"));
            }
            if *b == b'\n' {
                if i > 0 && bytes[i - 1] == b'\r' {
                    crlf = true;
                } else {
                    lf = true;
                }
                if crlf && lf {
                    return Err(Failure::new(
                        i,
                        "mixed LF/CRLF line endings are unsupported",
                    ));
                }
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
    fn rejects_unrepresentable_inputs() {
        for bytes in [&b"\xff"[..], b"A\0", b"A\rB", b"A\r\nB\n", b"\xff\xfeA\0"] {
            assert!(Source::from_bytes(bytes).is_err());
        }
        assert!(Source::from_bytes(&vec![b'a'; MAX_SOURCE_BYTES + 1]).is_err());
    }
}
