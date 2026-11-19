use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use tree_sitter::{Language, ParseOptions, Parser};
use tree_sitter_language::LanguageFn;

use crate::{
    lex::{self, Token},
    source::{Class, Failure, Source, Stage, Trace, Underlying},
};

/// Documented parser limits from `docs/parser-design.md`. They are named so a
/// test can assert the enforced value instead of repeating a literal.
pub const MAX_PARSE_BUDGET: Duration = Duration::from_secs(2);
pub const MAX_CST_DEPTH: usize = 512;
pub const MAX_FINGERPRINT_EVENTS: usize = 1_000_000;

/// The budget check as a pure predicate: "has this parse already taken longer
/// than the documented budget?". Keeping it free of `Instant` makes the
/// 2-second boundary testable without sleeping, so no test has to depend on
/// wall-clock timing to prove the rule.
fn budget_exceeded(elapsed: Duration, budget: Duration) -> bool {
    elapsed > budget
}

/// Effective resource limits for one bounded parse.
#[derive(Clone, Copy, Debug)]
struct Limits {
    cst_depth: usize,
    fingerprint_events: usize,
    parse_budget: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cst_depth: MAX_CST_DEPTH,
            fingerprint_events: MAX_FINGERPRINT_EVENTS,
            parse_budget: MAX_PARSE_BUDGET,
        }
    }
}

unsafe extern "C" {
    fn tree_sitter_verse() -> *const ();
}

#[derive(Debug)]
pub struct Document {
    pub tokens: Vec<Token>,
    pub spaced_operators: HashSet<usize>,
    pub call_openers: HashSet<usize>,
    pub spaced_for_separators: HashSet<usize>,
    shape: Vec<(u16, bool)>,
}

const EMPTY_ARCHETYPE_BRACE_MESSAGE: &str =
    "an empty indented archetype may have absorbed a braced expression on the next line";

fn empty_archetype_brace_continuation(
    node: tree_sitter::Node<'_>,
    source: &Source,
) -> Option<usize> {
    if node.kind() != "object_construction" {
        return None;
    }
    let head = node.child_by_field_name("type")?;
    if head.kind() != "object_construction" || head.child_by_field_name("fields").is_some() {
        return None;
    }
    let mut head_children = head.walk();
    let colon = head
        .children(&mut head_children)
        .find(|child| child.kind() == ":")?;
    let mut children = node.walk();
    let brace = node
        .children(&mut children)
        .find(|child| child.kind() == "{")?;
    (brace.start_position().row > colon.start_position().row)
        .then_some(source.body_start() + brace.start_byte())
}

const FOR_CLAUSE_BOUNDARY_MESSAGE: &str =
    "ambiguous clause boundary or postfix continuation in an indented for expression";

fn ambiguous_for_clause_boundary(
    node: tree_sitter::Node<'_>,
    bytes: &[u8],
    in_header: bool,
) -> Option<usize> {
    if node.kind() == "for_block_clause_list" {
        let mut cursor = node.walk();
        let mut previous = None::<tree_sitter::Node<'_>>;
        for child in node.children(&mut cursor).filter(|child| !child.is_extra()) {
            if child.kind() == "," {
                previous = None;
            } else {
                if let Some(left) = previous {
                    let left_text = bytes[left.byte_range()].trim_ascii_end();
                    let end = left.start_byte() + left_text.len();
                    if !bytes[end..child.start_byte()].contains(&b'\n') {
                        return Some(child.start_byte());
                    }
                }
                previous = Some(child);
            }
        }
        return None;
    }
    // The pinned grammar can attach the start of the next clause to a
    // previous expression as a postfix. Check the operator's own source span
    // so parentheses inside an explicitly delimited argument stay supported.
    let (field, opener) = match node.kind() {
        "call_expression" => ("function", "("),
        "failable_call_expression" => ("function", "["),
        "index_expression" => ("object", "["),
        "object_construction" | "map_literal" => ("type", "{"),
        _ => return None,
    };
    let left = node.child_by_field_name(field)?;
    if !in_header
        && !left
            .child_by_field_name("clauses")
            .is_some_and(|clauses| clauses.kind() == "for_block_clause_list")
    {
        return None;
    }
    let mut cursor = node.walk();
    let next = node
        .children(&mut cursor)
        .find(|child| child.kind() == opener)?;
    let end = left.start_byte() + bytes[left.byte_range()].trim_ascii_end().len();
    bytes[end..next.start_byte()]
        .contains(&b'\n')
        .then_some(next.start_byte())
}

impl Document {
    /// Parse without collecting a trace; the crate's own tests use this, the
    /// binary always needs the trace that `--explain` prints.
    #[cfg(test)]
    pub fn parse(source: &Source) -> Result<Self, Failure> {
        Self::parse_traced(source, &mut Trace::default())
    }

    /// Parse while recording the stages this source actually entered.
    ///
    /// The adjacent-structure guard runs before the ERROR/MISSING walk, exactly
    /// as it always has, so it stays the primary refusal for a source that also
    /// contains a raw parse error. What changed is only the reporting: the walk
    /// still runs, and the first raw `ERROR`/`MISSING` node it finds is attached
    /// to the guard refusal as a secondary, underlying cause. A guard refusal
    /// never becomes an acceptance, and a raw parse error is never reported as
    /// evidence that the guard is wrong.
    pub fn parse_traced(source: &Source, trace: &mut Trace) -> Result<Self, Failure> {
        Self::parse_with(source, trace, Limits::default())
    }

    /// The bounded walk itself, with the limits passed in.
    ///
    /// Production always uses [`Limits::default`], i.e. the documented values.
    /// The parameter exists so a test can prove the boundary of a limit that
    /// cannot be reached under the 250,000-token cap without generating a
    /// million-node source, and so the two-second parse budget can be exercised
    /// without depending on wall-clock timing. Passing the documented value is
    /// never optional in production: `parse_traced` hard-codes it.
    fn parse_with(source: &Source, trace: &mut Trace, limits: Limits) -> Result<Self, Failure> {
        trace.enter(Stage::Lexer);
        let tokens = lex::scan(source)?;
        trace.enter(Stage::Parser);
        // SAFETY: the function is linked from the pinned generated grammar and
        // returns a static TSLanguage with ABI 15, supported by tree-sitter 0.25.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| Failure::new(Stage::Parser, 0, format!("grammar ABI failure: {e}")))?;
        let bytes = &source.text().as_bytes()[source.body_start()..];
        let started = Instant::now();
        let mut cancel =
            |_: &tree_sitter::ParseState| budget_exceeded(started.elapsed(), limits.parse_budget);
        let tree = parser
            .parse_with_options(
                &mut |i, _| bytes.get(i..).unwrap_or_default(),
                None,
                Some(ParseOptions::new().progress_callback(&mut cancel)),
            )
            .ok_or_else(|| {
                Failure::new(
                    Stage::Parser,
                    0,
                    "parse budget exceeded; source was not changed",
                )
            })?;
        // The pinned grammar can split an inline function's X+Y into a body X
        // plus a sibling unary +Y without ERROR. Refuse unseparated top-level
        // statements on one physical line instead of trusting that recovery.
        //
        // The refusal is recorded rather than returned, so the walk below can
        // report what else is wrong with the same tree without changing this
        // guard's decision or its precedence.
        let mut guard: Option<Failure> = None;
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
                        guard = Some(Failure::guarded(
                            start,
                            "ambiguous adjacent top-level statements; inline binary function bodies are unsupported by the pinned grammar",
                        ));
                        break;
                    }
                }
            }
            previous = Some(node);
        }
        // A guard refusal never needs the fingerprint, and skipping it keeps a
        // guard-refused source from being reclassified by a resource limit.
        let fingerprint = guard.is_none();
        let mut shape = Vec::new();
        let mut spaced_operators = HashSet::new();
        let mut call_openers = HashSet::new();
        let mut spaced_for_separators = HashSet::new();
        // A recovery-free tree can still interpret <#...#> as operators and
        // line comments. Every independent lexical block comment must have
        // the same complete span in the CST before any output or write.
        // Strings/interpolation are opaque lexical tokens, not entries here.
        let mut unmatched_comments: HashSet<_> = tokens
            .iter()
            .filter(|token| token.kind == lex::Kind::BlockComment)
            .map(|token| (token.range.start, token.range.end))
            .collect();
        let mut cursor = tree.walk();
        let mut depth = 0;
        let mut for_header_depth = None;
        loop {
            let node = cursor.node();
            if node.kind() == "block_comment" {
                unmatched_comments.remove(&(
                    node.start_byte() + source.body_start(),
                    node.end_byte() + source.body_start(),
                ));
            }
            // Newlines are not separators in the pinned grammar, so a statement
            // line that starts with unary `+`/`-` can be absorbed into the
            // previous line as a binary operand without ERROR. Refuse instead of
            // re-spacing an operator whose owner is uncertain.
            if guard.is_none()
                && node.kind() == "binary_expression"
                && let (Some(left), Some(operator)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("operator"),
                )
                && matches!(operator.kind(), "+" | "-")
            {
                // A dedent can put trailing newline bytes in the left node.
                // Scan from its last non-whitespace byte to the operator.
                let bytes = source.text().as_bytes();
                let left_start = left.start_byte() + source.body_start();
                let left_end = left.end_byte() + source.body_start();
                let content_end = left_start + bytes[left_start..left_end].trim_ascii_end().len();
                let start = operator.start_byte() + source.body_start();
                if bytes[content_end..start].contains(&b'\n') {
                    guard = Some(Failure::guarded(
                        start,
                        "binary `+`/`-` operator starts a new line; a leading unary operand is ambiguous with a continued expression in the pinned grammar",
                    ));
                }
            }
            if node.kind() == "for_block_clause_list" && for_header_depth.is_none() {
                for_header_depth = Some(depth);
            }
            if guard.is_none()
                && let Some(offset) =
                    ambiguous_for_clause_boundary(node, bytes, for_header_depth.is_some())
            {
                guard = Some(Failure::guarded(
                    source.body_start() + offset,
                    FOR_CLAUSE_BOUNDARY_MESSAGE,
                ));
            }
            if guard.is_none()
                && let Some(start) = empty_archetype_brace_continuation(node, source)
            {
                guard = Some(Failure::guarded(start, EMPTY_ARCHETYPE_BRACE_MESSAGE));
            }
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
            ) || (node.kind() == "type_definition"
                && node.child_by_field_name("effect_specifiers").is_some())
            {
                let mut children = node.walk();
                for child in node.children(&mut children) {
                    if matches!(child.kind(), "(" | "[") {
                        call_openers.insert(child.start_byte() + source.body_start());
                    }
                }
            }
            if node.kind() == "for_iterator" {
                let mut children = node.walk();
                for child in node.children(&mut children) {
                    if child.kind() == ":" {
                        spaced_for_separators.insert(child.start_byte() + source.body_start());
                    }
                }
            }
            if node.is_error() || node.is_missing() {
                let offset = node.start_byte() + source.body_start();
                let node_kind = if node.is_missing() {
                    format!("MISSING {}", node.kind())
                } else {
                    node.kind().to_owned()
                };
                let Some(primary) = guard.take() else {
                    return Err(Failure::parse(
                        offset,
                        "unsupported or incomplete Verse syntax; use UEFN for compiler diagnostics",
                    ));
                };
                return Err(primary.with_underlying(Underlying {
                    stage: Stage::Parser,
                    class: Class::Parse,
                    offset,
                    message: format!(
                        "underlying parse error: {node_kind} node at byte {offset}; unsupported or incomplete Verse syntax",
                    ),
                    node_kind,
                }));
            }
            if depth > limits.cst_depth {
                if let Some(primary) = guard.take() {
                    return Err(primary);
                }
                return Err(Failure::new(
                    Stage::Parser,
                    node.start_byte() + source.body_start(),
                    "syntax tree resource limit exceeded",
                ));
            }
            if fingerprint {
                if shape.len() >= limits.fingerprint_events {
                    if let Some(primary) = guard.take() {
                        return Err(primary);
                    }
                    return Err(Failure::new(
                        Stage::Parser,
                        node.start_byte() + source.body_start(),
                        "syntax tree resource limit exceeded",
                    ));
                }
                shape.push((node.kind_id(), true));
            }
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                if fingerprint {
                    shape.push((cursor.node().kind_id(), false));
                }
                if for_header_depth == Some(depth) {
                    for_header_depth = None;
                }
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    if let Some(primary) = guard.take() {
                        return Err(primary);
                    }
                    if let Some((start, _)) = unmatched_comments.into_iter().min() {
                        return Err(Failure::new(
                            Stage::Parser,
                            start,
                            "unsupported block comment boundary: lexer and CST disagree; source was not changed",
                        ));
                    }
                    return Ok(Self {
                        tokens,
                        shape,
                        spaced_operators,
                        call_openers,
                        spaced_for_separators,
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
#[path = "range-binding-tests.rs"]
mod range_binding_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Source, Document) {
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let document = Document::parse(&source).unwrap();
        (source, document)
    }

    #[test]
    fn the_guard_stays_primary_and_the_raw_parse_error_is_secondary() {
        // A self-authored shape: an inline top-level binary function body (the
        // guard's own reason) plus markup the base grammar refuses, so the same
        // tree carries a raw ERROR. Before this slice only the guard message
        // was reported and the parse error surfaced only after a fix.
        let text = "Add(X:int,Y:int):int = X+Y\nText:=<p>Hello</p>\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let failure = Document::parse(&source).unwrap_err();
        assert_eq!(failure.stage, Stage::Parser);
        assert_eq!(failure.class, Some(Class::Guard));
        assert!(
            failure
                .message
                .contains("ambiguous adjacent top-level statements"),
            "{}",
            failure.message
        );
        assert_eq!(failure.offset, text.find("+Y").unwrap());
        assert_eq!(failure.underlying.len(), 1, "{failure:?}");
        let underlying = &failure.underlying[0];
        assert_eq!(underlying.stage, Stage::Parser);
        assert_eq!(underlying.class, Class::Parse);
        assert_eq!(underlying.node_kind, "ERROR");
        assert_eq!(underlying.offset, text.find("<p>").unwrap());
        assert!(
            underlying.message.contains("underlying parse error"),
            "{}",
            underlying.message
        );
        // The guard stays a refusal: an underlying parse error does not make a
        // guarded source acceptable.
        assert!(crate::format::format(&source, &crate::format::Options::default()).is_err());
    }

    #[test]
    fn a_missing_node_is_reported_with_its_kind_as_a_secondary_failure() {
        // An empty import list makes the base grammar insert a zero-width
        // MISSING identifier, so the secondary report names that kind instead
        // of pretending every raw finding is an ERROR node.
        let text = "A := 1 B := 2\nusing { }\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let failure = Document::parse(&source).unwrap_err();
        assert_eq!(failure.class, Some(Class::Guard));
        assert_eq!(failure.underlying[0].node_kind, "MISSING identifier");
        assert_eq!(failure.underlying[0].class, Class::Parse);
        assert!(
            failure.underlying[0]
                .message
                .contains("MISSING identifier node at byte"),
            "{}",
            failure.underlying[0].message
        );
    }

    #[test]
    fn a_guard_refusal_without_a_raw_parse_error_reports_no_secondary_entry() {
        // The guard alone refuses this: the tree has no ERROR or MISSING node,
        // so no secondary entry may be fabricated to look like one.
        for text in [
            "Add(X:int,Y:int):int = X+Y\n",
            "A := 1 B := 2\n",
            "Print(\"a\") Print(\"b\")\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let failure = Document::parse(&source).unwrap_err();
            assert_eq!(failure.class, Some(Class::Guard), "{text}");
            assert!(failure.underlying.is_empty(), "{text}: {failure:?}");
        }
    }

    #[test]
    fn a_line_starting_with_unary_plus_or_minus_is_refused_not_joined() {
        for text in [
            "F():void =\n    X := A\n    -2 < A\n",
            "F()<suspends>:void =\n    spawn:\n        G()\n    -2\n",
            "F()<suspends>:void =\r\n    spawn:\r\n        G()\r\n    +2\r\n",
            "F():void =\n    if:\n        A := B[]\n        -2 < A\n    then:\n        G()\n",
            "F():void =\n    case(T):\n        t.A =>\n            X := A\n            +1\n        _ => {}\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let failure = Document::parse(&source).unwrap_err();
            assert_eq!(failure.class, Some(Class::Guard), "{text}");
            assert!(
                failure.message.contains("starts a new line"),
                "{text}: {failure:?}"
            );
        }
        // Same-line binary operators and a unary operand that starts its own
        // statement after a separator keep parsing.
        for text in [
            "F():void =\n    X := A - 2\n",
            "F():void =\n    X := A\n    Y := -2\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_ok(), "{text}");
        }
    }

    #[test]
    fn a_raw_parse_error_without_the_guard_stays_a_parse_refusal() {
        let text = "Text:=<p>Hello</p>\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let failure = Document::parse(&source).unwrap_err();
        assert_eq!(failure.stage, Stage::Parser);
        assert_eq!(failure.class, Some(Class::Parse));
        assert!(failure.underlying.is_empty());
    }

    #[test]
    fn the_guard_refuses_exactly_the_sources_it_refused_before() {
        // Pre-slice refusal set: every one of these failed the guard with no
        // raw parse error. None may become accepted, and none may gain a
        // fabricated secondary entry.
        for text in [
            "Add(X:int,Y:int):int = X+Y\n",
            "F(X:int):int = X*2\nG(Y:int):int = Y-1\n",
            "A := 1 B := 2\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let failure = Document::parse(&source).unwrap_err();
            assert_eq!(failure.class, Some(Class::Guard), "{text}");
        }
        // A semicolon-separated pair is not what the guard is about: the base
        // grammar itself refuses it as a raw parse error, which is exactly why
        // the guard cannot be judged from the guard message alone.
        let separated = Source::from_bytes(b"A := 1; B := 2\n").unwrap();
        let failure = Document::parse(&separated).unwrap_err();
        assert_eq!(failure.class, Some(Class::Parse));
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
    fn indented_if_condition_keeps_each_line_as_its_own_item() {
        let text = "F():void =\n    if:\n        Value := Values[0]\n        set Seen[Value] = true\n        not Blocked[Value]\n    then:\n        Print(\"ok\")\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert!(!tree.root_node().has_error());
        let mut pending = vec![tree.root_node()];
        let mut condition = None;
        while let Some(node) = pending.pop() {
            if node.kind() == "if_block_condition" {
                condition = Some(node);
                break;
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        let condition = condition.unwrap();
        let mut cursor = condition.walk();
        let items: Vec<_> = condition
            .named_children(&mut cursor)
            .map(|node| node.utf8_text(text.as_bytes()).unwrap().to_owned())
            .collect();
        assert_eq!(
            items,
            [
                "Value",
                "Values[0]",
                "set Seen[Value] = true",
                "not Blocked[Value]"
            ]
        );
    }

    #[test]
    fn spawn_body_keeps_only_its_expression() {
        let text = "F()<suspends>:void =\n    spawn:\n        G()\n    H()\n    spawn{I()}\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());
        assert_eq!(root.named_child_count(), 1);
        let function = root.named_child(0).unwrap();
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(body.named_child_count(), 3);
        for (index, expected) in [(0, "G()"), (2, "I()")] {
            let spawn = body.named_child(index).unwrap();
            assert_eq!(spawn.kind(), "spawn_expression");
            let call = spawn.named_child(0).unwrap();
            assert_eq!(call.kind(), "call_expression");
            assert_eq!(call.utf8_text(text.as_bytes()).unwrap(), expected);
        }
        assert_eq!(
            body.named_child(1)
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "H()"
        );
    }

    #[test]
    fn case_arm_bodies_keep_their_statements() {
        let text = "F(T:t):void =\n    case(T):\n        t.A =>\n            C := 1\n            G(C)\n        t.B => {}\n        _ => H()\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert!(!tree.root_node().has_error());
        let mut pending = vec![tree.root_node()];
        let mut bodies = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == "case_arm" {
                let body = node.child_by_field_name("body").unwrap();
                bodies.push((
                    body.kind().to_owned(),
                    body.named_child_count(),
                    body.utf8_text(text.as_bytes()).unwrap().trim().to_owned(),
                ));
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        bodies.sort();
        assert_eq!(
            bodies,
            [
                ("brace_block".to_owned(), 0, "{}".to_owned()),
                ("call_expression".to_owned(), 1, "H()".to_owned()),
                (
                    "indented_block".to_owned(),
                    2,
                    "C := 1\n            G(C)".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn set_condition_and_key_value_iterator_keep_their_owners() {
        let text = "F():void =\n    if (set Values[Index] = Value):\n        Print(\"ok\")\n    for (Key -> Value : Values):\n        Print(Value)\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let mut pending = vec![tree.root_node()];
        let mut nodes = Vec::new();
        while let Some(node) = pending.pop() {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
            nodes.push(node);
        }

        let set = nodes
            .iter()
            .find(|node| node.kind() == "set_statement")
            .unwrap();
        assert_eq!(set.parent().unwrap().kind(), "if_condition");
        assert_eq!(
            set.child_by_field_name("target")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "Values[Index]"
        );

        let iterator = nodes
            .iter()
            .find(|node| node.kind() == "for_iterator")
            .unwrap();
        assert_eq!(iterator.parent().unwrap().kind(), "for_clause_list");
        for (field, expected) in [("key", "Key"), ("value", "Value"), ("iterable", "Values")] {
            assert_eq!(
                iterator
                    .child_by_field_name(field)
                    .unwrap()
                    .utf8_text(text.as_bytes())
                    .unwrap(),
                expected,
                "field {field}"
            );
        }
    }

    #[test]
    fn tuple_type_is_owned_by_the_archetype_head() {
        let text = "c := class:\n    E:event(tuple(int, logic)) = event(tuple(int, logic)){}\n    N:int = 1\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        let class = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(class.named_child_count(), 2);
        let field = class.named_child(0).unwrap();
        let declared_type = field.child_by_field_name("type").unwrap();
        assert_eq!(
            declared_type
                .child_by_field_name("argument")
                .unwrap()
                .kind(),
            "tuple_type"
        );
        let initializer = field.child_by_field_name("default").unwrap();
        assert_eq!(initializer.kind(), "object_construction");
        let head = initializer.child_by_field_name("type").unwrap();
        assert_eq!(head.kind(), "call_expression");
        let arguments = head.child_by_field_name("arguments").unwrap();
        assert_eq!(arguments.named_child_count(), 1);
        assert_eq!(arguments.named_child(0).unwrap().kind(), "tuple_type");
        assert_eq!(class.named_child(1).unwrap().kind(), "field_declaration");
    }

    #[test]
    fn indented_for_retains_clauses_body_and_sibling() {
        let text = "F():void=\n    for:\n        Key -> Item : Items\n        Choice := Pick[Key]\n        Choice > 0\n    do:\n        Use(Item, Choice)\n    Done()\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        parser
            .set_language(&Language::new(unsafe {
                LanguageFn::from_raw(tree_sitter_verse)
            }))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 2);
        let expression = body.named_child(0).unwrap();
        assert_eq!(expression.kind(), "for_expression");
        let clauses = expression.child_by_field_name("clauses").unwrap();
        assert_eq!(clauses.kind(), "for_block_clause_list");
        let mut cursor = clauses.walk();
        assert_eq!(
            clauses
                .named_children(&mut cursor)
                .map(|child| child.kind())
                .collect::<Vec<_>>(),
            ["for_iterator", "for_filter_binding", "binary_expression"]
        );
        assert_eq!(
            expression.child_by_field_name("body").unwrap().kind(),
            "colon_block"
        );
        assert_eq!(body.named_child(1).unwrap().kind(), "call_expression");
    }

    #[test]
    fn indented_for_refuses_ambiguous_clause_boundaries() {
        for joined in ["(0)", "[0]", "{Value := 0}"] {
            let text = format!(
                "F():void =\n    for:\n        Item : Items\n        {joined}\n    do:\n        Use(Item)\n"
            );
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let error = Document::parse(&source).unwrap_err();
            assert!(
                error.message.contains(FOR_CLAUSE_BOUNDARY_MESSAGE),
                "{joined}: {error:?}"
            );
        }
    }

    #[test]
    fn indented_for_result_refuses_next_line_index() {
        let text = "F():void =\n    Selection := for:\n        Item : Items\n    do:\n        Item\n    [0]\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let error = Document::parse(&source).unwrap_err();
        assert!(
            error.message.contains(FOR_CLAUSE_BOUNDARY_MESSAGE),
            "{error:?}"
        );
    }

    #[test]
    fn for_filter_binding_owns_its_value_and_keeps_body_separate() {
        let text =
            "F():void =\n    for (I -> S : Ss, U := Us[I], U > 0):\n        G(S, U)\n    H()\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 2);
        let loop_node = body.named_child(0).unwrap();
        let clauses = loop_node.child_by_field_name("clauses").unwrap();
        assert_eq!(clauses.named_child_count(), 3);
        assert_eq!(clauses.named_child(0).unwrap().kind(), "for_iterator");
        let binding = clauses.named_child(1).unwrap();
        assert_eq!(binding.kind(), "for_filter_binding");
        assert_eq!(
            binding
                .child_by_field_name("name")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "U"
        );
        assert_eq!(
            binding
                .child_by_field_name("value")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "Us[I]"
        );
        assert_eq!(clauses.named_child(2).unwrap().kind(), "binary_expression");
        assert_eq!(
            loop_node
                .child_by_field_name("body")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(
            body.named_child(1)
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "H()"
        );
    }

    #[test]
    fn constructor_owns_archetype_without_reclassifying_a_type_function() {
        let text = "Make<public><constructor>(Old:item)<transacts> := item:\n    Count := Old.Count\nAlias(T:type) := T\nOther():int = 1\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        assert_eq!(root.named_child_count(), 3);
        let constructor = root.named_child(0).unwrap();
        assert_eq!(constructor.kind(), "type_definition");
        assert_eq!(
            constructor
                .child_by_field_name("type_params")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(
            constructor
                .child_by_field_name("effect_specifiers")
                .unwrap()
                .named_child_count(),
            1
        );
        let body = constructor.child_by_field_name("value").unwrap();
        assert_eq!(body.kind(), "object_construction");
        assert_eq!(
            body.child_by_field_name("fields")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(root.named_child(1).unwrap().kind(), "type_definition");
        assert_eq!(root.named_child(2).unwrap().kind(), "function_definition");
    }

    #[test]
    fn commas_in_indented_enum_keep_variants_and_next_definition_separate() {
        let text = "shade := enum:\n    Light,\n    Grey\n    Dark,\nNext:int = 1\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        assert_eq!(root.named_child_count(), 2);
        let variants = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(variants.kind(), "enum_definition");
        assert_eq!(variants.named_child_count(), 3);
        for (index, name) in ["Light", "Grey", "Dark"].into_iter().enumerate() {
            let variant = variants.named_child(index).unwrap();
            assert_eq!(variant.kind(), "enum_variant");
            assert_eq!(
                variant
                    .child_by_field_name("name")
                    .unwrap()
                    .utf8_text(text.as_bytes())
                    .unwrap(),
                name
            );
        }
        assert_eq!(root.named_child(1).unwrap().kind(), "constant_declaration");
    }

    #[test]
    fn spaced_effect_specifiers_stay_on_the_signature() {
        let text = "F(N:int) <transacts><decides>:int =\n    F(0) < transacts > G(1)\n    N\nA := F(0) < G(1)\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        assert_eq!(root.named_child_count(), 2);
        let function = root.named_child(0).unwrap();
        assert_eq!(function.kind(), "function_definition");
        let list = function.child_by_field_name("effect_specifiers").unwrap();
        assert_eq!(list.kind(), "specifier_list");
        assert_eq!(list.named_child_count(), 2);
        for specifier in [list.named_child(0).unwrap(), list.named_child(1).unwrap()] {
            assert_eq!(specifier.kind(), "specifier");
        }
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(body.named_child_count(), 2);
        let first_statement = body.named_child(0).unwrap();
        assert_eq!(first_statement.kind(), "binary_expression");
        assert_eq!(
            first_statement
                .child_by_field_name("operator")
                .unwrap()
                .kind(),
            "<"
        );
        assert_eq!(
            body.named_child(1)
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "N"
        );
        let comparison = root
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(comparison.kind(), "binary_expression");
        assert_eq!(
            comparison.child_by_field_name("operator").unwrap().kind(),
            "<"
        );
    }

    #[test]
    fn empty_archetype_ends_before_its_sibling_statement() {
        let text = "F():item =\n    A:item = item:\n    B:item = item:\n        Count := 1\n    return A\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 3);
        let empty = body
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(empty.kind(), "object_construction");
        assert!(empty.child_by_field_name("fields").is_none());
        let filled = body
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(filled.kind(), "object_construction");
        assert_eq!(
            filled
                .child_by_field_name("fields")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(body.named_child(2).unwrap().kind(), "return_expression");
    }

    #[test]
    fn refuses_expression_continuations_after_an_empty_indented_archetype() {
        for (suffix, message) in [
            ("{Count := 1}", EMPTY_ARCHETYPE_BRACE_MESSAGE),
            ("-2", "a leading unary operand is ambiguous"),
        ] {
            let text = format!("F():void =\n    A:item = item:\n    {suffix}\n");
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let refusal = Document::parse(&source).expect_err("continuation must refuse");
            assert!(refusal.message.contains(message), "{refusal:?}");
        }
    }

    #[test]
    fn map_pairs_keep_their_keys_values_and_following_statement() {
        let text =
            "F():void =\n    Pairs := map{Key => Value, Other => 2}\n    Empty := map{}\n    G()\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());

        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 3);
        let pairs = body
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(pairs.kind(), "map_literal");
        assert_eq!(pairs.named_child_count(), 3);
        for (index, key, value) in [(1, "Key", "Value"), (2, "Other", "2")] {
            let entry = pairs.named_child(index).unwrap();
            assert_eq!(entry.kind(), "map_entry");
            for (field, expected) in [("key", key), ("value", value)] {
                assert_eq!(
                    entry
                        .child_by_field_name(field)
                        .unwrap()
                        .utf8_text(text.as_bytes())
                        .unwrap(),
                    expected
                );
            }
        }
        let empty = body
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(empty.kind(), "object_construction");
        assert_eq!(body.named_child(2).unwrap().kind(), "call_expression");
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
            "A := map{\"x\" => }\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_err(), "{text}");
        }
    }

    /// The documented parse budget is two seconds, and the budget must not be
    /// tested with a real clock: a loaded runner may parse a small source in
    /// under a millisecond or over two seconds. The predicate is exercised
    /// directly, at the boundary and one nanosecond past it.
    #[test]
    fn the_parse_budget_is_two_seconds_and_is_compared_exactly() {
        assert_eq!(MAX_PARSE_BUDGET, Duration::from_secs(2));
        assert!(!budget_exceeded(Duration::ZERO, MAX_PARSE_BUDGET));
        assert!(!budget_exceeded(MAX_PARSE_BUDGET, MAX_PARSE_BUDGET));
        assert!(budget_exceeded(
            MAX_PARSE_BUDGET + Duration::from_nanos(1),
            MAX_PARSE_BUDGET
        ));
    }

    /// The CST depth bound is 512. A chain of 510 additions measures depth 512
    /// and is accepted; 511 additions measure 513 and are refused. The depth
    /// limit is also injectable, so the seam is exercised at depth 2 vs 3 to
    /// prove the comparison is `>` against the configured bound.
    #[test]
    fn cst_depth_limit_is_512_and_accepts_its_own_boundary() {
        assert_eq!(MAX_CST_DEPTH, 512);
        let text = format!("A := 1{}\n", " + 1".repeat(510));
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let document = Document::parse(&source).unwrap();
        assert!(document.shape.len() > 3_000, "{}", document.shape.len());

        let text = format!("A := 1{}\n", " + 1".repeat(511));
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let failure = Document::parse(&source).unwrap_err();
        assert_eq!(failure.stage, Stage::Parser);
        assert!(
            failure
                .message
                .contains("syntax tree resource limit exceeded"),
            "{}",
            failure.message
        );

        // The reduced seam: a flat source is shallow, so a bound of 1 must
        // refuse it while the documented 512 accepts it.
        let flat = Source::from_bytes(b"A := 1\n").unwrap();
        assert!(
            Document::parse_with(
                &flat,
                &mut Trace::default(),
                Limits {
                    cst_depth: 1,
                    ..Limits::default()
                }
            )
            .is_err()
        );
        assert!(Document::parse(&flat).is_ok());
    }

    /// The fingerprint-event bound is 1,000,000 node-enter plus node-exit events.
    /// The 250,000-token cap keeps a real source far under that bound (the
    /// densest reachable source observes well under 400,000 events), so the
    /// boundary is proven through the reduced `Limits` seam on the same walk.
    /// The literal constant is asserted too, so a changed value fails this test
    /// even if the seam stays green.
    ///
    /// Technique: reduced constant behind a private test-visible seam
    /// (`Limits::fingerprint_events`), because the documented 1,000,000 is not
    /// reachable within another documented limit.
    ///
    /// Observed actual behaviour, reported in the pull request rather than
    /// changed here: the bound is compared only when a node is **entered**, so
    /// `shape` can overshoot the configured value by at most the tree depth
    /// before the refusal fires. With a limit of 7 on `A := 1\n` the walk
    /// records 10 events and still succeeds; with a limit of 6 — the length
    /// `shape` has when its last node is entered — the walk refuses.
    #[test]
    fn fingerprint_event_limit_is_one_million_and_bounds_the_walk() {
        assert_eq!(MAX_FINGERPRINT_EVENTS, 1_000_000);
        let source = Source::from_bytes(b"A := 1\n").unwrap();
        let events = Document::parse(&source).unwrap().shape.len();
        assert_eq!(events, 10);

        // The last entry happens when `shape` already holds 6 events, so that is
        // the smallest limit that refuses this source.
        let accepted = Document::parse_with(
            &source,
            &mut Trace::default(),
            Limits {
                fingerprint_events: 7,
                ..Limits::default()
            },
        )
        .unwrap();
        assert_eq!(accepted.shape.len(), events);

        let refused = Document::parse_with(
            &source,
            &mut Trace::default(),
            Limits {
                fingerprint_events: 6,
                ..Limits::default()
            },
        )
        .unwrap_err();
        assert_eq!(refused.stage, Stage::Parser);
        assert!(
            refused
                .message
                .contains("syntax tree resource limit exceeded"),
            "{}",
            refused.message
        );

        // A large source stays under the documented bound: 125,000 lines are
        // 250,000 tokens, and the observed event count for that shape is far
        // below 1,000,000.
        let dense = Source::from_bytes("A\n".repeat(125_000).as_bytes()).unwrap();
        let document = Document::parse(&dense).unwrap();
        assert!(document.shape.len() < MAX_FINGERPRINT_EVENTS);
    }
}
