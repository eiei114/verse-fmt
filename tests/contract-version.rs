#[test]
fn documents_shared_toolchain_contract_version_one() {
    let cli_docs = include_str!("../docs/cli.md");
    assert!(cli_docs.contains("Shared toolchain contract version **1**."));
}
