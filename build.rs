use std::process::Command;

/// Sets `BUILD_VERSION` to the package version plus the git commit, so a
/// running instance shows exactly what it was built from.
fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let build_version = match git_revision() {
        Some(revision) => format!("{version} ({revision})"),
        // E.g. when built from a source tarball.
        None => version.to_string(),
    };
    println!("cargo:rustc-env=BUILD_VERSION={build_version}");
}

/// The short commit hash, with `-dirty` for uncommitted changes.
fn git_revision() -> Option<String> {
    // HEAD and its reflog change on commit and checkout, the index on
    // staging, and src on edits.
    let git_dir = git(&["rev-parse", "--absolute-git-dir"])?;
    for path in ["HEAD", "logs/HEAD", "index"] {
        println!("cargo:rerun-if-changed={git_dir}/{path}");
    }
    println!("cargo:rerun-if-changed=src");

    // `--exclude=*` skips the tags, so the output is only the hash.
    git(&["describe", "--always", "--dirty", "--exclude=*"])
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_string())
}
