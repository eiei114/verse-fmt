use serde::{Deserialize, Serialize};

use crate::{
    indent,
    lex::{Kind, Token},
    source::{Failure, Source},
    syntax::Document,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    Preserve,
    Lf,
    Crlf,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Options {
    pub line_ending: LineEnding,
}

pub fn format(source: &Source, options: &Options) -> Result<String, Failure> {
    let before = Document::parse(source)?;
    let indentation = indent::analyze(source, &before.tokens)?;
    let output = layout(source, &before, &indentation, options)?;
    let output_source = Source::from_bytes(output.as_bytes())?;
    let after = Document::parse(&output_source)?;
    let after_indentation = indent::analyze(&output_source, &after.tokens)?;
    if !before.equivalent(source, &after, &output_source)
        || indentation.signature != after_indentation.signature
    {
        return Err(Failure::new(
            0,
            "formatting failed token/structure/indentation preservation checks; source was not changed",
        ));
    }
    if layout(&output_source, &after, &after_indentation, options)? != output {
        return Err(Failure::new(
            0,
            "formatting failed idempotence check; source was not changed",
        ));
    }
    Ok(output)
}

fn layout(
    source: &Source,
    document: &Document,
    indentation: &indent::Indentation,
    options: &Options,
) -> Result<String, Failure> {
    let newline = match options.line_ending {
        LineEnding::Preserve => source.newline(),
        LineEnding::Lf => "\n",
        LineEnding::Crlf => "\r\n",
    };
    let mut output = String::with_capacity(source.text().len());
    let mut skipped = indentation.blank_lines.iter().peekable();
    for (index, token) in document.tokens.iter().enumerate() {
        while skipped.peek().is_some_and(|r| r.end <= token.range.start) {
            skipped.next();
        }
        if skipped
            .peek()
            .is_some_and(|r| r.start <= token.range.start && token.range.end <= r.end)
        {
            continue;
        }
        let previous = index.checked_sub(1).map(|i| &document.tokens[i]);
        let next = document.tokens.get(index + 1);
        if token.kind == Kind::Newline {
            output.push_str(if options.line_ending == LineEnding::Preserve {
                token.text(source)
            } else {
                newline
            });
            continue;
        }
        if !token.kind.is_trivia()
            && options.line_ending != LineEnding::Preserve
            && contains_other_line_ending(token.text(source), newline)
        {
            return Err(Failure::new(
                token.range.start,
                "requested line ending would change a protected comment/string",
            ));
        }
        if token.kind == Kind::Space {
            if next.is_none_or(|t| t.kind == Kind::Newline) {
                continue;
            }
            if let Some(indent) = indentation.replacements.get(&token.range.start) {
                output.push_str(indent);
                continue;
            }
            if let (Some(left), Some(right)) = (previous, next)
                && let Some(gap) = gap(source, document, left, right)
            {
                output.push_str(gap);
                continue;
            }
            output.push_str(token.text(source));
            continue;
        }
        if let Some(left) = previous
            && let Some(gap) = gap(source, document, left, token)
        {
            output.push_str(gap);
        }
        output.push_str(token.text(source));
    }
    if output.len() > source.body_start() && !output.ends_with('\n') {
        output.push_str(newline);
    }
    Ok(output)
}

fn contains_other_line_ending(text: &str, requested: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(i, byte)| {
        *byte == b'\n'
            && if i > 0 && bytes[i - 1] == b'\r' {
                requested != "\r\n"
            } else {
                requested != "\n"
            }
    })
}

fn gap(source: &Source, document: &Document, left: &Token, right: &Token) -> Option<&'static str> {
    if left.kind.is_trivia()
        || right.kind.is_trivia()
        || left.kind.is_comment()
        || right.kind.is_comment()
    {
        return None;
    }
    let a = left.text(source);
    let b = right.text(source);
    if document.spaced_for_separators.contains(&left.range.start)
        || document.spaced_for_separators.contains(&right.range.start)
    {
        return Some(" ");
    }
    if assignment(a)
        || assignment(b)
        || document.spaced_operators.contains(&left.range.start)
        || document.spaced_operators.contains(&right.range.start)
    {
        return Some(" ");
    }
    if a == ":"
        || b == ":"
        || matches!(a, "(" | "[")
        || matches!(b, ")" | "]" | "," | ";")
        || document.call_openers.contains(&right.range.start)
    {
        return Some("");
    }
    if matches!(a, "," | ";") {
        return Some(" ");
    }
    None
}

fn assignment(text: &str) -> bool {
    matches!(text, "=" | ":=" | "+=" | "-=" | "*=" | "/=")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_idempotent_and_preserves_protected_bytes() {
        for text in [
            "",
            "\u{feff}",
            "Count:=1  ",
            "<# outer <# inner #> #>\nX:=1\n",
            "Name:=\"abc  \" # keep  \n",
            "# comment",
            "Count:=1\r\n",
            "calc := class:\n  Value : int=1\n  Get ( X : int , Y : int ) : int=\n    X+Y\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let once = format(&source, &Options::default()).unwrap();
            let twice = format(
                &Source::from_bytes(once.as_bytes()).unwrap(),
                &Options::default(),
            )
            .unwrap();
            assert_eq!(once, twice, "{text}");
        }
    }

    #[test]
    fn normalizes_confirmed_spaced_blocks() {
        let source = Source::from_bytes(
            b"calc := class:\n  Value : int=1\n  Get ( X : int , Y : int ) : int=\n    X+Y\n",
        )
        .unwrap();
        assert_eq!(
            format(&source, &Options::default()).unwrap(),
            "calc := class:\n    Value:int = 1\n    Get(X:int, Y:int):int =\n        X + Y\n"
        );
    }

    #[test]
    fn preserves_each_mixed_line_ending_and_uses_last_style_for_added_final_newline() {
        let source = Source::from_bytes(b"A:=1\r\nB:=2\nC:=3  ").unwrap();
        assert_eq!(
            format(&source, &Options::default()).unwrap(),
            "A := 1\r\nB := 2\nC := 3\n"
        );
    }

    #[test]
    fn forced_line_ending_rejects_only_protected_spans_that_would_change() {
        let source = Source::from_bytes(b"<# same style\r\n#>\r\nA:=1\n").unwrap();
        assert!(
            format(
                &source,
                &Options {
                    line_ending: LineEnding::Crlf
                }
            )
            .is_ok()
        );
        let mixed_protected = Source::from_bytes(b"<# mixed\r\nLF\n#>\nA:=1\n").unwrap();
        assert!(
            format(
                &mixed_protected,
                &Options {
                    line_ending: LineEnding::Crlf
                }
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_line_ending_changes_inside_protected_spans() {
        let source = Source::from_bytes(b"<# a\nb #>\nX:=1\n").unwrap();
        assert!(
            format(
                &source,
                &Options {
                    line_ending: LineEnding::Crlf
                }
            )
            .is_err()
        );
    }
}
