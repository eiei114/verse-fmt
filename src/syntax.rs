use std::time::{Duration, Instant};

use tree_sitter::{Language, ParseOptions, Parser};
use tree_sitter_language::LanguageFn;

use crate::{
    lex::{self, Token},
    source::{Failure, Source},
};

unsafe extern "C" {
    fn tree_sitter_verse() -> *const ();
}

#[derive(Debug)]
pub struct Document {
    pub tokens: Vec<Token>,
    shape: Vec<(u16, bool)>,
}

impl Document {
    pub fn parse(source: &Source) -> Result<Self, Failure> {
        let tokens = lex::scan(source)?;
        // SAFETY: the function is linked from the pinned generated grammar and
        // returns a static TSLanguage with ABI 15, supported by tree-sitter 0.25.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| Failure::new(0, format!("grammar ABI failure: {e}")))?;
        let bytes = &source.text().as_bytes()[source.body_start()..];
        let started = Instant::now();
        let mut cancel = |_: &tree_sitter::ParseState| started.elapsed() > Duration::from_secs(2);
        let tree = parser
            .parse_with_options(
                &mut |i, _| bytes.get(i..).unwrap_or_default(),
                None,
                Some(ParseOptions::new().progress_callback(&mut cancel)),
            )
            .ok_or_else(|| Failure::new(0, "parse budget exceeded; source was not changed"))?;
        let mut shape = Vec::new();
        let mut cursor = tree.walk();
        let mut depth = 0;
        loop {
            let node = cursor.node();
            if node.is_error() || node.is_missing() {
                return Err(Failure::new(
                    node.start_byte() + source.body_start(),
                    "unsupported or incomplete Verse syntax; use UEFN for compiler diagnostics",
                ));
            }
            if depth > 512 || shape.len() >= 1_000_000 {
                return Err(Failure::new(
                    node.start_byte() + source.body_start(),
                    "syntax tree resource limit exceeded",
                ));
            }
            shape.push((node.kind_id(), true));
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                shape.push((cursor.node().kind_id(), false));
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(Self { tokens, shape });
                }
                depth -= 1;
            }
        }
    }

    pub fn equivalent(&self, source: &Source, other: &Self, output: &Source) -> bool {
        self.shape == other.shape
            && self
                .tokens
                .iter()
                .filter(|t| !t.kind.is_trivia())
                .map(|t| (t.kind, t.text(source)))
                .eq(other
                    .tokens
                    .iter()
                    .filter(|t| !t.kind.is_trivia())
                    .map(|t| (t.kind, t.text(output))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Source, Document) {
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let document = Document::parse(&source).unwrap();
        (source, document)
    }

    #[test]
    fn recognizes_different_indent_widths_without_calling_them_invalid() {
        let (a, ad) = parsed("calc := class:\n  Value:int = 1\n  Get():int =\n    Value\n");
        let (b, bd) = parsed("calc := class:\n    Value:int = 1\n    Get():int =\n        Value\n");
        assert!(ad.equivalent(&a, &bd, &b));
    }

    #[test]
    fn catches_changes_to_block_ownership_with_identical_code_tokens() {
        let (a, ad) = parsed(
            "calc := class:\n    First():void =\n        Print(\"one\")\n    Second():void =\n        Print(\"two\")\n",
        );
        let (b, bd) = parsed(
            "calc := class:\n    First():void =\n        Print(\"one\")\nSecond():void =\n    Print(\"two\")\n",
        );
        assert!(!ad.equivalent(&a, &bd, &b));
    }

    #[test]
    fn guards_recovery_and_unsupported_constructs() {
        for text in [
            "<# missing",
            "<#> comment\n    body\nCount := 1\n",
            "A := <p>text</p>\n",
            "得点 := 1\n",
            "A:int=1\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_err(), "{text}");
        }
    }
}
