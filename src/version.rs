use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, sync::OnceLock};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Identifies the running artifact, including builds with uncommitted changes.
/// Read once with bounded memory; no Git checkout or external tools are needed.
pub fn build_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| fingerprint().unwrap_or_else(|_| "unknown".into()))
}

fn fingerprint() -> std::io::Result<String> {
    let mut file = File::open(std::env::current_exe()?)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(hex::encode(hash.finalize())[..12].into())
}
