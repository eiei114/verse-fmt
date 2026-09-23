//! Original structural expectations, not compiler acceptance or private project code.
use super::*;
use tree_sitter::{Node, Tree};

fn tree(text: &str) -> Tree {
    let source = Source::from_bytes(text.as_bytes()).unwrap();
    Document::parse(&source).unwrap();
    let mut parser = Parser::new();
    // SAFETY: the statically linked production grammar.
    let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
    parser.set_language(&language).unwrap();
    let tree = parser.parse(text, None).unwrap();
    assert!(!tree.root_node().has_error());
    tree
}

fn nodes<'a>(root: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut pending = vec![root];
    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == kind {
            found.push(node);
        }
        pending.extend(node.named_children(&mut node.walk()));
    }
    found.sort_by_key(Node::start_byte);
    found
}

fn field<'a>(node: Node<'_>, name: &str, source: &'a str) -> &'a str {
    node.child_by_field_name(name)
        .unwrap()
        .utf8_text(source.as_bytes())
        .unwrap()
}

#[test]
fn range_binding_keeps_bounds_precedence_and_filter_outside_iterable() {
    let text = "F():void =\n    for (I := -2..Limit + 3 * 2, I <> 0):\n        Print(I)\n";
    let tree = tree(text);
    let iterators = nodes(tree.root_node(), "for_iterator");
    assert_eq!(iterators.len(), 1);
    let iterator = iterators[0];
    assert_eq!(field(iterator, "variable", text), "I");
    assert_eq!(field(iterator, "iterable", text), "-2..Limit + 3 * 2");
    let range = iterator.child_by_field_name("iterable").unwrap();
    assert_eq!(range.kind(), "range_expression");
    assert_eq!(field(range, "start", text), "-2");
    assert_eq!(field(range, "end", text), "Limit + 3 * 2");
    let end = range.child_by_field_name("end").unwrap();
    assert_eq!(field(end, "left", text), "Limit");
    assert_eq!(field(end, "operator", text), "+");
    assert_eq!(field(end, "right", text), "3 * 2");
    let clauses = iterator.parent().unwrap();
    assert_eq!(clauses.kind(), "for_clause_list");
    assert_eq!(clauses.named_child_count(), 2);
    assert_eq!(
        clauses
            .named_child(1)
            .unwrap()
            .utf8_text(text.as_bytes())
            .unwrap(),
        "I <> 0"
    );
}

#[test]
fn nested_range_bodies_do_not_capture_siblings_or_ordinary_bindings() {
    let text = "F():void =\n    Seed := 2\n    for (I := 1..3):\n        for (J := 4..6):\n            Print(\"inner\")\n        Print(\"outer\")\n    Print(\"after\")\nG():void =\n    Print(\"sibling\")\n";
    let tree = tree(text);
    let root = tree.root_node();
    assert_eq!(root.named_child_count(), 2);
    let functions = nodes(root, "function_definition");
    assert_eq!(functions.len(), 2);
    assert_eq!(field(functions[0], "name", text), "F");
    assert_eq!(field(functions[1], "name", text), "G");
    let iterators = nodes(root, "for_iterator");
    assert_eq!(iterators.len(), 2);
    assert_eq!(field(iterators[0], "variable", text), "I");
    assert_eq!(field(iterators[1], "variable", text), "J");
    // The vendored grammar calls untyped := declarations type_definition.
    let declarations = nodes(root, "type_definition");
    assert_eq!(declarations.len(), 1);
    assert_eq!(
        declarations[0].utf8_text(text.as_bytes()).unwrap(),
        "Seed := 2"
    );
    assert_eq!(field(declarations[0], "name", text), "Seed");
    assert_eq!(field(declarations[0], "value", text), "2");
    let calls = nodes(root, "call_expression");
    assert_eq!(calls.len(), 4);
    for (call, (expected_text, expected_loops, function)) in calls.iter().zip([
        ("Print(\"inner\")", 2, "F"),
        ("Print(\"outer\")", 1, "F"),
        ("Print(\"after\")", 0, "F"),
        ("Print(\"sibling\")", 0, "G"),
    ]) {
        assert_eq!(call.utf8_text(text.as_bytes()).unwrap(), expected_text);
        let mut parent = call.parent();
        let mut loops = 0;
        let mut owner = None;
        while let Some(node) = parent {
            loops += usize::from(node.kind() == "for_expression");
            if node.kind() == "function_definition" {
                owner = Some(field(node, "name", text));
            }
            parent = node.parent();
        }
        assert_eq!(loops, expected_loops, "{expected_text}");
        assert_eq!(owner, Some(function), "{expected_text}");
    }
}

#[test]
fn range_binding_clause_order_and_parenthesized_bounds_are_explicit() {
    let text = "F():void =\n    for (I := (Start + 1)..(End * 2), J := 4..6, J <> I):\n        Print(I + J)\n";
    let tree = tree(text);
    let iterators = nodes(tree.root_node(), "for_iterator");
    assert_eq!(iterators.len(), 2);
    for (iterator, (variable, start, end)) in iterators
        .iter()
        .zip([("I", "(Start + 1)", "(End * 2)"), ("J", "4", "6")])
    {
        assert_eq!(field(*iterator, "variable", text), variable);
        let range = iterator.child_by_field_name("iterable").unwrap();
        assert_eq!(range.kind(), "range_expression");
        assert_eq!(field(range, "start", text), start);
        assert_eq!(field(range, "end", text), end);
    }
    let clauses = iterators[0].parent().unwrap();
    assert_eq!(iterators[1].parent(), Some(clauses));
    assert_eq!(clauses.named_child_count(), 3);
    assert_eq!(clauses.named_child(0), Some(iterators[0]));
    assert_eq!(clauses.named_child(1), Some(iterators[1]));
    assert_eq!(
        clauses
            .named_child(2)
            .unwrap()
            .utf8_text(text.as_bytes())
            .unwrap(),
        "J <> I"
    );
}

#[test]
fn incomplete_ranges_and_unimplemented_intermediate_bindings_fail_closed() {
    for clause in [
        "I :=",
        "I := 1..",
        "I := ..3",
        ":= 1..3",
        "I := 1..3,",
        "I := Values",
        "I := 1..3, Twice := I * 2",
    ] {
        let text = format!("F():void =\n    for ({clause}):\n        Print(\"x\")\n");
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        assert!(Document::parse(&source).is_err(), "{clause}");
    }
}
