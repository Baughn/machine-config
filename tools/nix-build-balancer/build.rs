use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );

    let mut files: Vec<PathBuf> = Vec::new();
    collect(&root.join("src"), &mut files);
    collect(&root.join("tests"), &mut files);
    push_if_exists(&root.join("Cargo.toml"), &mut files);
    push_if_exists(&root.join("build.rs"), &mut files);
    files.sort();

    let mut hasher = Sha256::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update([0u8]);
        let bytes = fs::read(file).expect("hashed input file readable");
        hasher.update(&bytes);
        hasher.update([0u8]);
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    println!("cargo:rustc-env=NBB_SOURCE_HASH={hex}");
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

fn push_if_exists(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path.to_path_buf());
    }
}
