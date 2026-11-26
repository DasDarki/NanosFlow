#[path = "src/snapshot.rs"]
#[allow(dead_code)]
mod snapshot;

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest directory")).join("../../resources/api");
    println!("cargo:rerun-if-changed={}", root.join(snapshot::REVISION_FILE).display());
    println!("cargo:rerun-if-env-changed=NANOSFLOW_API_REVISION");
    let complete = ["latest", "stable"].iter().all(|channel| root.join(channel).join("APIFiles.json").is_file());
    if complete && std::env::var("NANOSFLOW_API_REVISION").is_err() {
        return;
    }
    let revision = std::env::var("NANOSFLOW_API_REVISION")
        .or_else(|_| snapshot::latest_revision())
        .unwrap_or_else(|error| panic!("The nanos world API definitions are missing and could not be downloaded: {error}"));
    let staging = root.with_file_name(format!(".api-download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).expect("the staging folder can be created");
    if let Err(error) = snapshot::download(&revision, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        panic!("The nanos world API {revision} could not be downloaded from {}: {error}", snapshot::REPOSITORY);
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::rename(&staging, &root).expect("the downloaded API can be moved into resources/api");
}
