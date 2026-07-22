//! Write `zoeken-client/src/lib/generated/native.ts` from native wire DTOs.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let default_out = manifest_dir
        .join("../../zoeken-client/src/lib/generated/native.ts")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../zoeken-client/src/lib/generated/native.ts"));
    let out = env::args().nth(1).map(PathBuf::from).unwrap_or(default_out);

    if let Some(parent) = out.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!("failed to create {}: {error}", parent.display());
            return ExitCode::FAILURE;
        }
    }

    let body = zoeken_server::native::export_typescript();
    if let Err(error) = fs::write(&out, body) {
        eprintln!("failed to write {}: {error}", out.display());
        return ExitCode::FAILURE;
    }
    println!("wrote {}", out.display());
    ExitCode::SUCCESS
}
