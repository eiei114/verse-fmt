fn main() {
    let source = "vendor/tree-sitter-verse/src";
    println!("cargo:rerun-if-changed={source}");
    cc::Build::new()
        .include(source)
        .define("TREE_SITTER_HIDE_SYMBOLS", None)
        .file(format!("{source}/parser.c"))
        .file(format!("{source}/scanner.c"))
        .warnings(false)
        .compile("tree-sitter-verse");
}
