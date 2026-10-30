//! One build identity for every surface: Web header, `/v1/status`, logs and
//! the layout cache. It is derived from Git at compile time, so it changes
//! with every commit without anyone editing a number, and it is the same for
//! every architecture built from one commit.
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, sync::OnceLock};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIBE: &str = env!("CF_GATEWAY_DESCRIBE");
/// Release tags on this branch look like `cf-v0.2.14`.
pub const TAG_PREFIX: &str = "cf-v";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Cargo.toml version, changed only by a release.
    pub version: String,
    /// `0.2.14` on the release commit, `0.2.14+3.g79a09af99` three commits
    /// later, plus `.dirty.<binary hash>` for uncommitted local changes.
    pub build: String,
    pub commit: Option<String>,
    /// Commits since the last `cf-v*` tag; `None` when no tag is reachable.
    pub distance: Option<u32>,
    pub dirty: bool,
}

/// Parse `git describe --tags --match cf-v* --long --dirty --always` output
/// (or the same value supplied by CI). `binary_hash` is only used to
/// distinguish dirty builds, which share a commit but not their code.
pub fn parse(version: &str, describe: &str, binary_hash: impl FnOnce() -> String) -> Identity {
    let mut describe = describe.trim();
    let dirty = describe.ends_with("-dirty");
    if dirty {
        describe = &describe[..describe.len() - "-dirty".len()];
    }
    let (mut commit, mut distance, mut metadata) = (None, None, Vec::new());
    // cf-v<tag version>-<distance>-g<commit>, or a bare commit without a tag.
    let tagged = describe.strip_prefix(TAG_PREFIX).and_then(|rest| {
        let (head, sha) = rest.rsplit_once("-g")?;
        let (tag_version, count) = head.rsplit_once('-')?;
        let count = count.parse::<u32>().ok()?;
        (is_hex(sha) && !tag_version.is_empty()).then_some((tag_version, count, sha))
    });
    if let Some((tag_version, count, sha)) = tagged {
        commit = Some(sha.to_string());
        distance = Some(count);
        if count != 0 || tag_version != version {
            metadata.push(format!("{count}.g{sha}"));
        }
    } else if is_hex(describe) {
        commit = Some(describe.to_string());
        metadata.push(format!("g{describe}"));
    } else {
        metadata.push("unknown".into());
    }
    if dirty {
        metadata.push(format!("dirty.{}", binary_hash()));
    }
    Identity {
        version: version.to_string(),
        build: if metadata.is_empty() {
            version.to_string()
        } else {
            format!("{version}+{}", metadata.join("."))
        },
        commit,
        distance,
        dirty,
    }
}
fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.len() <= 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn identity() -> &'static Identity {
    static IDENTITY: OnceLock<Identity> = OnceLock::new();
    IDENTITY.get_or_init(|| parse(VERSION, DESCRIBE, || binary_hash()[..8].to_string()))
}
/// The complete build string shown everywhere.
pub fn build() -> &'static str {
    &identity().build
}
/// SHA-256 prefix of the running executable. Differs per architecture and
/// per compilation, so it is diagnostic data rather than the identity.
pub fn binary_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| fingerprint().unwrap_or_else(|_| "unknown".into()))
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

#[cfg(test)]
mod tests {
    use super::*;
    fn hash() -> String {
        "abcdef12".into()
    }
    #[test]
    fn release_commit_is_the_plain_version_and_later_commits_count_up() {
        let release = parse("0.2.14", "cf-v0.2.14-0-g8a5bd88f0", hash);
        assert_eq!(release.build, "0.2.14");
        assert_eq!(release.commit.as_deref(), Some("8a5bd88f0"));
        assert_eq!(release.distance, Some(0));
        assert!(!release.dirty);
        let later = parse("0.2.14", "cf-v0.2.14-3-g79a09af99", hash);
        assert_eq!(later.build, "0.2.14+3.g79a09af99");
        assert_eq!(later.distance, Some(3));
    }
    #[test]
    fn untagged_bumped_or_dirty_checkouts_stay_distinguishable() {
        // Cargo.toml bumped without a tag: the tag version is not trusted.
        let bumped = parse("0.2.15", "cf-v0.2.14-0-g8a5bd88f0", hash);
        assert_eq!(bumped.build, "0.2.15+0.g8a5bd88f0");
        let dirty = parse("0.2.14", "cf-v0.2.14-3-g79a09af99-dirty", hash);
        assert_eq!(dirty.build, "0.2.14+3.g79a09af99.dirty.abcdef12");
        assert!(dirty.dirty && dirty.commit.as_deref() == Some("79a09af99"));
        let dirty_release = parse("0.2.14", "cf-v0.2.14-0-g8a5bd88f0-dirty", hash);
        assert_eq!(dirty_release.build, "0.2.14+dirty.abcdef12");
    }
    #[test]
    fn no_tag_and_no_git_fall_back_without_inventing_a_distance() {
        let bare = parse("0.2.14", "79a09af99", hash);
        assert_eq!(bare.build, "0.2.14+g79a09af99");
        assert_eq!(bare.distance, None);
        let none = parse("0.2.14", "", hash);
        assert_eq!(none.build, "0.2.14+unknown");
        assert_eq!(none.commit, None);
        let garbage = parse("0.2.14", "cf-vx-y-gnothex", hash);
        assert_eq!(garbage.build, "0.2.14+unknown");
        let app_tag = parse("0.2.14", "3.20.050310-12-gabcdef123", hash);
        assert_eq!(app_tag.build, "0.2.14+unknown");
    }
    #[test]
    fn running_binary_reports_a_parsable_identity() {
        let current = identity();
        assert_eq!(current.version, VERSION);
        assert!(current.build.starts_with(VERSION));
        assert_eq!(binary_hash().len(), 12);
    }
}
