fn main() {
    // Embed git commit hash at compile time
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=FLYNT_BUILD_HASH={}", hash.trim());
    println!("cargo:rerun-if-changed=.git/HEAD");
}
