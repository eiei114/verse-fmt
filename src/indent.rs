//! Independent line/indentation guard; no inferred tab widths or newline rewriting.
use std::{collections::BTreeMap, ops::Range};

use crate::{
    lex::{Kind, Token},
    source::{Failure, MAX_NESTING, Source},
};

#[derive(Debug)]
pub struct Indentation {
    pub replacements: BTreeMap<usize, String>,
    pub blank_lines: Vec<Range<usize>>,
    pub signature: Vec<(usize, usize)>,
}

pub fn analyze(source: &Source, tokens: &[Token]) -> Result<Indentation, Failure> {
    let mut result = Indentation {
        replacements: BTreeMap::new(),
        blank_lines: Vec::new(),
        signature: Vec::new(),
    };
    let mut widths = vec![0];
    let mut brackets = 0usize;
    let mut previous_code = "";
    let (mut offset, mut token_index, mut blank_count) = (0, 0, 0);
    for line in source.text().split_inclusive('\n') {
        let end = offset + line.len();
        let start = if offset == 0 {
            source.body_start()
        } else {
            offset
        };
        let content = start
            + source.text()[start..end]
                .bytes()
                .take_while(|b| matches!(b, b' ' | b'\t'))
                .count();
        while token_index < tokens.len() && tokens[token_index].range.end <= start {
            token_index += 1;
        }
        let mut stop = token_index;
        while stop < tokens.len() && tokens[stop].range.start < end {
            stop += 1;
        }
        let line_tokens = &tokens[token_index..stop];
        let protected_start = line_tokens.first().is_some_and(|t| t.range.start < start);
        let significant: Vec<_> = line_tokens
            .iter()
            .filter(|t| !t.kind.is_trivia() && !t.kind.is_comment())
            .collect();
        let code_line = !protected_start && !significant.is_empty();
        let blank = !protected_start && line_tokens.iter().all(|t| t.kind.is_trivia());
        if blank {
            blank_count += 1;
            if blank_count > 1 {
                result.blank_lines.push(start..end);
            }
        } else {
            blank_count = 0;
        }
        if code_line && brackets == 0 {
            if source.text()[start..content].contains('\t') {
                return Err(Failure::new(
                    start,
                    "tab indentation cannot be safely normalized; replace it explicitly first",
                ));
            }
            let width = content - start;
            let current = *widths.last().unwrap();
            if width > current {
                if !matches!(previous_code, ":" | "=" | ":=" | "=>") {
                    return Err(Failure::new(
                        content,
                        "unsupported indentation continuation; source was not changed",
                    ));
                }
                widths.push(width);
                if widths.len() > MAX_NESTING {
                    return Err(Failure::new(content, "indentation exceeds 256 levels"));
                }
            } else {
                while width < *widths.last().unwrap() {
                    widths.pop();
                }
                if width != *widths.last().unwrap() {
                    return Err(Failure::new(
                        content,
                        "inconsistent dedent; use UEFN to check the source",
                    ));
                }
            }
            let level = widths.len() - 1;
            result.signature.push((level, 0));
            if width != level * 4 {
                result.replacements.insert(start, " ".repeat(level * 4));
            }
        } else if code_line {
            // Delimited continuation/brace indentation is preserved exactly.
            result.signature.push((content - start, brackets));
        } else if !blank
            && !protected_start
            && brackets == 0
            && let Some(level) = widths.iter().position(|w| *w == content - start)
            && content - start != level * 4
        {
            result.replacements.insert(start, " ".repeat(level * 4));
        }
        for token in significant {
            previous_code = token.text(source);
            if token.kind != Kind::Code {
                continue;
            }
            match previous_code {
                "(" | "[" | "{" => brackets += 1,
                ")" | "]" | "}" => brackets = brackets.saturating_sub(1),
                _ => (),
            }
        }
        offset = end;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex;

    #[test]
    fn indentation_signature_catches_moved_statements() {
        let a = Source::from_bytes(b"f():void =\n  A()\n  B()\n").unwrap();
        let b = Source::from_bytes(b"f():void =\n  A()\nB()\n").unwrap();
        assert_ne!(
            analyze(&a, &lex::scan(&a).unwrap()).unwrap().signature,
            analyze(&b, &lex::scan(&b).unwrap()).unwrap().signature
        );
    }

    #[test]
    fn refuses_guessed_indent() {
        for text in [
            "A()\n  B()\n",
            "f():void =\n\tA()\n",
            "f():void =\n    A()\n  B()\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(
                analyze(&source, &lex::scan(&source).unwrap()).is_err(),
                "{text}"
            );
        }
    }
}
