/// Every misuse of `#[derive(Schema)]` fails to compile with a message that
/// points at the mistake. Regenerate the expected output with
/// `TRYBUILD=overwrite cargo test --test derive` and review the diff.
#[test]
fn derive_errors() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
