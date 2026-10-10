use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(|| "unknown".to_string(), |o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    println!("cargo:rustc-env=JQPLAY_GIT_SHA={sha}");
    println!("cargo:rerun-if-changed=build.rs");
    if let Ok(o) = Command::new("git").args(["rev-parse", "--git-path", "HEAD"]).output() {
        println!("cargo:rerun-if-changed={}", String::from_utf8_lossy(&o.stdout).trim());
    }
}
