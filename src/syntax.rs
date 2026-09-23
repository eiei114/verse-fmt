use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

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
    pub spaced_operators: HashSet<usize>,
    pub call_openers: HashSet<usize>,
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
        // The pinned grammar can split an inline function's X+Y into a body X
        // plus a sibling unary +Y without ERROR. Refuse unseparated top-level
        // statements on one physical line instead of trusting that recovery.
        let mut children = tree.root_node().walk();
        let mut previous: Option<tree_sitter::Node<'_>> = None;
        for node in tree.root_node().named_children(&mut children) {
            if node.is_extra() {
                continue;
            }
            if let Some(left) = previous {
                let end = left.end_byte() + source.body_start();
                let start = node.start_byte() + source.body_start();
                if source.position(end.saturating_sub(1)).0 == source.position(start).0 {
                    let first = tokens.partition_point(|t| t.range.end <= end);
                    let separated = tokens[first..]
                        .iter()
                        .take_while(|t| t.range.start < start)
                        .any(|t| t.kind == lex::Kind::Code && t.text(source) == ";");
                    if !separated {
                        return Err(Failure::new(
                            start,
                            "ambiguous adjacent top-level statements; inline binary function bodies are unsupported by the pinned grammar",
                        ));
                    }
                }
            }
            previous = Some(node);
        }
        let mut shape = Vec::new();
        let mut spaced_operators = HashSet::new();
        let mut call_openers = HashSet::new();
        let mut cursor = tree.walk();
        let mut depth = 0;
        loop {
            let node = cursor.node();
            if matches!(node.kind(), "binary_expression" | "set_statement")
                && let Some(operator) = node.child_by_field_name("operator")
            {
                spaced_operators.insert(operator.start_byte() + source.body_start());
            }
            if matches!(
                node.kind(),
                "call_expression"
                    | "failable_call_expression"
                    | "index_expression"
                    | "function_definition"
                    | "extension_function_definition"
            ) {
                let mut children = node.walk();
                for child in node.children(&mut children) {
                    if matches!(child.kind(), "(" | "[") {
                        call_openers.insert(child.start_byte() + source.body_start());
                    }
                }
            }
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
                    return Ok(Self {
                        tokens,
                        shape,
                        spaced_operators,
                        call_openers,
                    });
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
    fn typed_constants_retain_method_and_file_ownership() {
        let text = "sample := class:\n    First():void =\n        Label:string=\"hello\"\n        Count:int=2\n    Second():void =\n        Other:int=3\nOutside:int=4\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let mut pending = vec![tree.root_node()];
        let mut constants = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == "constant_declaration" {
                let name = node
                    .child_by_field_name("name")
                    .unwrap()
                    .utf8_text(text.as_bytes())
                    .unwrap();
                let parent = node.parent().unwrap();
                let scope = if parent.kind() == "source_file" {
                    "file"
                } else {
                    assert_eq!(parent.kind(), "indented_block");
                    let function = parent.parent().unwrap();
                    assert_eq!(function.kind(), "function_definition");
                    assert_eq!(function.parent().unwrap().kind(), "class_definition");
                    function
                        .child_by_field_name("name")
                        .unwrap()
                        .utf8_text(text.as_bytes())
                        .unwrap()
                };
                constants.push((name, scope));
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        constants.sort_unstable();
        assert_eq!(
            constants,
            [
                ("Count", "First"),
                ("Label", "First"),
                ("Other", "Second"),
                ("Outside", "file")
            ]
        );
    }

    #[test]
    fn guards_recovery_and_unsupported_constructs() {
        for text in [
            "<# missing",
            "<#> comment\n    body\nCount := 1\n",
            "A := <p>text</p>\n",
            "得点 := 1\n",
            "A:int=\n",
            "A:int\n",
            "using { Demo..Helpers }\n",
            "using { Demo. }\n",
            "using { }\n",
            "shade := enum{Light,,Dark}\n",
            "shade := enum{,Light}\n",
            "Add(X:int,Y:int):int = X+Y\n",
            "A := map{\"x\" => 1}\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_err(), "{text}");
        }
    }
}
