//! Deterministic, bounded property/fuzz-style checks. Not a compiler oracle.
use crate::{
    format::{self, Options},
    lex,
    source::Source,
    syntax::Document,
};

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn seeded_structured_inputs_keep_tokens_and_reach_independent_golden_layout() {
    let mut seed = 0x7665_7273_6566_6d74_u64;
    for _ in 0..256 {
        let width = (next(&mut seed) % 8 + 1) as usize;
        let value = next(&mut seed) % 10000;
        let bom = if next(&mut seed) & 1 == 0 {
            ""
        } else {
            "\u{feff}"
        };
        let newline = if next(&mut seed) & 1 == 0 {
            "\n"
        } else {
            "\r\n"
        };
        let pad = " ".repeat(width);
        let input=format!("{bom}counter:=class:\n{pad}Value : int={value}  \n{pad}Label:string=\"tag {value} 日😀\"\n{pad}Get ( X : int ) : int=\n{pad}{pad}X+Value  \n").replace('\n',newline);
        let expected=format!("{bom}counter := class:\n    Value:int = {value}\n    Label:string = \"tag {value} 日😀\"\n    Get(X:int):int =\n        X + Value\n").replace('\n',newline);
        let source = Source::from_bytes(input.as_bytes()).unwrap();
        let output = format::format(&source, &Options::default()).unwrap();
        assert_eq!(output, expected);
        let after = Source::from_bytes(output.as_bytes()).unwrap();
        assert!(Document::parse(&source).unwrap().equivalent(
            &source,
            &Document::parse(&after).unwrap(),
            &after
        ));
        assert_eq!(format::format(&after, &Options::default()).unwrap(), output);
    }
}

#[test]
fn seeded_arbitrary_bytes_never_panic_or_return_a_non_idempotent_success() {
    let mut seed = 0x7361_6665_6279_7465_u64;
    for case in 0..1024 {
        let len = (next(&mut seed) % 512) as usize;
        let bytes: Vec<_> = (0..len)
            .map(|_| {
                let n = next(&mut seed) as u8;
                if case % 2 == 0 { n % 128 } else { n }
            })
            .collect();
        let outcome = std::panic::catch_unwind(|| {
            if let Ok(source) = Source::from_bytes(&bytes) {
                if let Ok(tokens) = lex::scan(&source) {
                    let joined: String = tokens.iter().map(|t| t.text(&source)).collect();
                    assert_eq!(joined, source.text());
                }
                if let Ok(output) = format::format(&source, &Options::default()) {
                    assert_eq!(
                        format::format(
                            &Source::from_bytes(output.as_bytes()).unwrap(),
                            &Options::default()
                        )
                        .unwrap(),
                        output
                    );
                }
            }
        });
        assert!(outcome.is_ok(), "seeded case {case}: {bytes:?}");
    }
}

#[test]
fn deep_and_token_dense_sources_fail_bounded_while_large_literal_is_preserved() {
    let deep = format!("A := {}1{}\n", "(".repeat(257), ")".repeat(257));
    assert!(
        format::format(
            &Source::from_bytes(deep.as_bytes()).unwrap(),
            &Options::default()
        )
        .unwrap_err()
        .message
        .contains("nesting")
    );
    let dense = "# c\n".repeat(125001);
    assert!(
        format::format(
            &Source::from_bytes(dense.as_bytes()).unwrap(),
            &Options::default()
        )
        .unwrap_err()
        .message
        .contains("token")
    );
    let literal = format!("A:=\"{}\"\n", "x".repeat(1024 * 1024));
    assert_eq!(
        format::format(
            &Source::from_bytes(literal.as_bytes()).unwrap(),
            &Options::default()
        )
        .unwrap(),
        literal.replacen(":=", " := ", 1)
    );
}
