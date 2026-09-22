use crate::{
    lex::Kind,
    source::{Failure, Source},
    syntax::Document,
};

pub fn format(source: &Source) -> Result<String, Failure> {
    let before = Document::parse(source)?;
    let output = layout(source, &before);
    let output_source = Source::from_bytes(output.as_bytes())?;
    let after = Document::parse(&output_source)?;
    if !before.equivalent(source, &after, &output_source) {
        return Err(Failure::new(
            0,
            "formatting failed token/structure preservation checks; source was not changed",
        ));
    }
    if layout(&output_source, &after) != output {
        return Err(Failure::new(
            0,
            "formatting failed idempotence check; source was not changed",
        ));
    }
    Ok(output)
}

/// First supported slice: assignment spacing, safe trailing whitespace, final newline.
/// Indentation and all protected spans remain byte-identical.
fn layout(source: &Source, document: &Document) -> String {
    let mut output = String::with_capacity(source.text().len());
    for (index, token) in document.tokens.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| &document.tokens[i]);
        let next = document.tokens.get(index + 1);
        if token.kind == Kind::Space {
            // Whitespace before a newline/EOF is outside comments and literals.
            if next.is_none_or(|t| t.kind == Kind::Newline) {
                continue;
            }
            // Never touch leading indentation or whitespace next to comments.
            if previous.is_some_and(|t| !t.kind.is_trivia() && !t.kind.is_comment())
                && next.is_some_and(|t| !t.kind.is_trivia() && !t.kind.is_comment())
                && (previous.is_some_and(|t| assignment(t.text(source)))
                    || next.is_some_and(|t| assignment(t.text(source))))
            {
                output.push(' ');
            } else {
                output.push_str(token.text(source));
            }
            continue;
        }
        if !token.kind.is_trivia()
            && !token.kind.is_comment()
            && previous.is_some_and(|t| !t.kind.is_trivia() && !t.kind.is_comment())
            && (assignment(token.text(source))
                || previous.is_some_and(|t| assignment(t.text(source))))
        {
            output.push(' ');
        }
        output.push_str(token.text(source));
    }
    if output.len() > source.body_start() && !output.ends_with('\n') {
        output.push_str(source.newline());
    }
    output
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
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let once = format(&source).unwrap();
            let twice = format(&Source::from_bytes(once.as_bytes()).unwrap()).unwrap();
            assert_eq!(once, twice, "{text}");
        }
    }
}
