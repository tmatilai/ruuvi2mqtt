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
///
/// Works with git 2.7, which the cross 0.2.5 images have.
fn git_revision() -> Option<String> {
    // Rerun when a commit, checkout or staging changes HEAD, its reflog or
    // the index, and when src changes. git and Cargo both take a relative
    // git dir as relative to the package directory.
    let git_dir = git(&["rev-parse", "--git-dir"])?;
    for path in ["HEAD", "logs/HEAD", "index"] {
        println!("cargo:rerun-if-changed={git_dir}/{path}");
    }
    println!("cargo:rerun-if-changed=src");

    let hash = git(&["rev-parse", "--short", "HEAD"])?;
    // Untracked files don't count, as with `git describe --dirty`.
    let changes = git(&["status", "--porcelain", "--untracked-files=no"])?;
    Some(if changes.is_empty() {
        hash
    } else {
        format!("{hash}-dirty")
    })
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_string())
}
