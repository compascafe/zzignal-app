fn main() {
    // Force re-run on every commit (HEAD changes, logs/HEAD changes on pull/commit)
    println!("cargo:rerun-if-changed=../.git/logs/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/heads/main");
    println!("cargo:rerun-if-changed=../.git/HEAD");

    let git_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
