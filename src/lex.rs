//! Strict, lossless lexical guard independent of tree-sitter recovery.
use std::ops::Range;

use crate::source::{Failure, MAX_NESTING, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bom,
    Space,
    Newline,
    Code,
    String,
    Character,
    LineComment,
    BlockComment,
}

impl Kind {
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::Bom | Self::Space | Self::Newline)
    }
    pub fn is_comment(self) -> bool {
        matches!(self, Self::LineComment | Self::BlockComment)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
}

impl Token {
    pub fn text<'a>(&self, source: &'a Source) -> &'a str {
        &source.text()[self.range.clone()]
    }
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
}

impl Scanner<'_> {
    fn next(&self, i: usize) -> usize {
        i + self.text[i..].chars().next().unwrap().len_utf8()
    }
    fn starts(&self, i: usize, s: &str) -> bool {
        self.text[i..].starts_with(s)
    }
    fn guard(&self, i: usize, level: usize) -> Result<(), Failure> {
        if level > MAX_NESTING {
            Err(Failure::new(i, "nesting exceeds 256 levels"))
        } else {
            Ok(())
        }
    }

    fn block_comment(&self, start: usize, outer: usize) -> Result<usize, Failure> {
        let (mut i, mut level) = (start, 0);
        while i < self.bytes.len() {
            if self.starts(i, "<#>") {
                return Err(Failure::new(
                    i,
                    "indented <#> comments are not supported yet",
                ));
            }
            if self.starts(i, "<#") {
                level += 1;
                self.guard(i, outer + level)?;
                i += 2;
            } else if self.starts(i, "#>") {
                level -= 1;
                i += 2;
                if level == 0 {
                    return Ok(i);
                }
            } else {
                i = self.next(i);
            }
        }
        Err(Failure::new(start, "unterminated block comment"))
    }

    fn character(&self, start: usize) -> Result<usize, Failure> {
        let mut i = start + 1;
        if self.bytes.get(i) == Some(&b'\\') {
            i = self.escape(i)?;
        } else if i < self.bytes.len() && !matches!(self.bytes[i], b'\n' | b'\r' | b'\'') {
            i = self.next(i);
        } else {
            return Err(Failure::new(
                start,
                "unsupported or incomplete character literal",
            ));
        }
        if self.bytes.get(i) == Some(&b'\'') {
            Ok(i + 1)
        } else {
            Err(Failure::new(
                start,
                "unsupported quoted identifier or character literal",
            ))
        }
    }

    fn escape(&self, start: usize) -> Result<usize, Failure> {
        if self
            .bytes
            .get(start + 1)
            .is_some_and(|b| b"tnr\"'\\{}<>#~&".contains(b))
        {
            Ok(start + 2)
        } else {
            Err(Failure::new(
                start,
                "unsupported or incomplete escape sequence",
            ))
        }
    }

    fn string(&self, start: usize, level: usize) -> Result<usize, Failure> {
        self.guard(start, level)?;
        let mut i = start + 1;
        while i < self.bytes.len() {
            match self.bytes[i] {
                b'"' => return Ok(i + 1),
                b'\\' => i = self.escape(i)?,
                b'{' => i = self.interpolation(i + 1, level + 1)?,
                b'\n' | b'\r' => {
                    return Err(Failure::new(
                        i,
                        "multiline string literals are not supported yet",
                    ));
                }
                _ => i = self.next(i),
            }
        }
        Err(Failure::new(start, "unterminated string literal"))
    }

    fn interpolation(&self, mut i: usize, level: usize) -> Result<usize, Failure> {
        self.guard(i, level)?;
        let start = i;
        let mut delimiters = Vec::new();
        while i < self.bytes.len() {
            match self.bytes[i] {
                b'"' => i = self.string(i, level + delimiters.len() + 1)?,
                b'\'' => i = self.character(i)?,
                b'<' if self.starts(i, "<#") => {
                    i = self.block_comment(i, level + delimiters.len())?
                }
                b'#' => {
                    while i < self.bytes.len() && !matches!(self.bytes[i], b'\n' | b'\r') {
                        i = self.next(i);
                    }
                }
                b'(' | b'[' | b'{' => {
                    delimiters.push(self.bytes[i]);
                    self.guard(i, level + delimiters.len())?;
                    i += 1;
                }
                b')' | b']' | b'}' => {
                    if self.bytes[i] == b'}' && delimiters.is_empty() {
                        return Ok(i + 1);
                    }
                    if delimiters.pop() != Some(opener(self.bytes[i])) {
                        return Err(Failure::new(i, "unbalanced interpolation delimiters"));
                    }
                    i += 1;
                }
                _ => i = self.next(i),
            }
        }
        Err(Failure::new(start, "unterminated string interpolation"))
    }
}

fn opener(close: u8) -> u8 {
    match close {
        b')' => b'(',
        b']' => b'[',
        b'}' => b'{',
        _ => unreachable!(),
    }
}

pub fn scan(source: &Source) -> Result<Vec<Token>, Failure> {
    let scanner = Scanner {
        text: source.text(),
        bytes: source.text().as_bytes(),
    };
    let mut tokens = Vec::new();
    let mut delimiters = Vec::new();
    let mut i = source.body_start();
    if i > 0 {
        tokens.push(Token {
            kind: Kind::Bom,
            range: 0..i,
        });
    }
    while i < scanner.bytes.len() {
        if tokens.len() >= 250_000 {
            return Err(Failure::new(i, "source exceeds 250000 lexical tokens"));
        }
        let start = i;
        let kind = match scanner.bytes[i] {
            b' ' | b'\t' => {
                while scanner
                    .bytes
                    .get(i)
                    .is_some_and(|b| matches!(b, b' ' | b'\t'))
                {
                    i += 1;
                }
                Kind::Space
            }
            b'\r' | b'\n' => {
                i += if scanner.bytes[i] == b'\r' { 2 } else { 1 };
                Kind::Newline
            }
            b'#' => {
                while i < scanner.bytes.len() && !matches!(scanner.bytes[i], b'\n' | b'\r') {
                    i = scanner.next(i);
                }
                Kind::LineComment
            }
            b'<' if scanner.starts(i, "<#") => {
                i = scanner.block_comment(i, delimiters.len())?;
                Kind::BlockComment
            }
            b'"' => {
                i = scanner.string(i, delimiters.len() + 1)?;
                Kind::String
            }
            b'\'' => {
                i = scanner.character(i)?;
                Kind::Character
            }
            b'(' | b'[' | b'{' => {
                delimiters.push(scanner.bytes[i]);
                scanner.guard(i, delimiters.len())?;
                i += 1;
                Kind::Code
            }
            b')' | b']' | b'}' => {
                if delimiters.pop() != Some(opener(scanner.bytes[i])) {
                    return Err(Failure::new(i, "unbalanced delimiters"));
                }
                i += 1;
                Kind::Code
            }
            b if b.is_ascii_alphanumeric() || b == b'_' => {
                while scanner
                    .bytes
                    .get(i)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    i += 1;
                }
                Kind::Code
            }
            _ => {
                if let Some(pair) = [
                    ":=", "=>", "->", "<>", "<=", ">=", "+=", "-=", "*=", "/=", "..",
                ]
                .iter()
                .find(|s| scanner.starts(i, s))
                {
                    i += pair.len();
                } else {
                    let ch = scanner.text[i..].chars().next().unwrap();
                    if !ch.is_ascii() || !":=<>+-*/.,;?@&|!%^~".contains(ch) {
                        return Err(Failure::new(
                            i,
                            "unsupported source token (Unicode in literals/comments is supported)",
                        ));
                    }
                    i += 1;
                }
                Kind::Code
            }
        };
        tokens.push(Token {
            kind,
            range: start..i,
        });
    }
    if !delimiters.is_empty() {
        return Err(Failure::new(i, "unclosed delimiter"));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_nested_literals_and_comments() {
        let source = Source::from_bytes(
            "\u{feff}<# outer <# inner #> #>\r\nLabel:=\"日{Format(\"😀\")}\" # end  \r\n"
                .as_bytes(),
        )
        .unwrap();
        let tokens = scan(&source).unwrap();
        let rebuilt: String = tokens.iter().map(|t| t.text(&source)).collect();
        assert_eq!(rebuilt, source.text());
        assert_eq!(tokens.iter().filter(|t| t.kind == Kind::String).count(), 1);
    }

    #[test]
    fn rejects_truncated_protected_spans() {
        for text in [
            "<# missing",
            "<# nested <# #>",
            "\"missing",
            "\"{F(\"x\")\"",
            "<#> comment",
            "'ab'",
            "\"bad\\z\"",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(scan(&source).is_err(), "{text}");
        }
    }

    #[test]
    fn limits_nesting_including_interpolations() {
        for text in [
            format!("{}1{}", "(".repeat(257), ")".repeat(257)),
            format!("X := \"{{{}1{}}}\"", "(".repeat(257), ")".repeat(257)),
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(scan(&source).is_err());
        }
    }
}
