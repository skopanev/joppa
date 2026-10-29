// Part 1 of tests/access.rs, included there, so the test names stay unchanged.

/// The rule file from the environment, or a temporary one for this test binary only.
fn private_file(var: &str, name: &str) -> std::path::PathBuf {
    if let Some(path) = std::env::var_os(var) {
        return path.into();
    }
    let path = std::env::temp_dir().join(format!("joppa-test-{name}-{}.json", std::process::id()));
    // SAFETY: this binary has one test, and it sets the variable before it starts work.
    unsafe { std::env::set_var(var, &path) };
    path
}
