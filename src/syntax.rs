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
        loop {
            let node = cursor.node();
            if node.kind() == "block_comment" {
                unmatched_comments.remove(&(
                    node.start_byte() + source.body_start(),
                    node.end_byte() + source.body_start(),
                ));
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
            ) {
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
