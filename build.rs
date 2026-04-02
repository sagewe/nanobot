use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run this build script when any frontend source file changes.
    // Cargo watches these paths and only re-invokes build.rs when they differ.
    let sources = [
        "frontend/index.html",
        "frontend/src/main.js",
        "frontend/src/render.js",
        "frontend/src/style.css",
        "frontend/src/api.js",
        "frontend/src/i18n.js",
        "frontend/package.json",
    ];
    for src in &sources {
        println!("cargo:rerun-if-changed={src}");
    }

    let frontend_dir = Path::new("frontend");

    // Skip frontend build when the frontend directory is missing (e.g. in
    // downstream crate consumers or CI jobs that don't need it).
    if !frontend_dir.exists() {
        return;
    }

    // Run `npm install` only when node_modules is absent.
    if !frontend_dir.join("node_modules").exists() {
        let status = Command::new("npm")
            .args(["install"])
            .current_dir(frontend_dir)
            .status()
            .expect("failed to run npm install — is Node.js installed?");
        assert!(status.success(), "npm install failed");
    }

    // Run `npm run build` (vite) to produce frontend/dist/.
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(frontend_dir)
        .status()
        .expect("failed to run npm run build — is Node.js installed?");
    assert!(status.success(), "vite build failed");
}
