//! The CLI tests, one per case in `tests/cmd/`.

#[test]
fn cli_tests() {
    trycmd::TestCases::new().case("tests/cmd/*.toml");
}
