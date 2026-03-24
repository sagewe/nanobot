use std::process::Command;

fn main() {
    // Tell cargo to re-run if frontend source changes
    println!("cargo:rerun-if-changed=frontend/src/main.js");
    println!("cargo:rerun-if-changed=frontend/src/style.css");
    println!("cargo:rerun-if-changed=frontend/index.html");

    // Only build if dist doesn't exist or sources changed
    let dist = std::path::Path::new("frontend/dist");

    // Run npm install if node_modules missing
    if !std::path::Path::new("frontend/node_modules").exists() {
        let status = Command::new("npm")
            .args(["install"])
            .current_dir("frontend")
            .status()
            .expect("failed to run npm install");
        assert!(status.success(), "npm install failed");
    }

    // Run vite build
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir("frontend")
        .status()
        .expect("failed to run vite build");
    assert!(status.success(), "vite build failed");

    let _ = dist;
}
