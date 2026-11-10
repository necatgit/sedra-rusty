pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// SEDRA: the release of this Sedra node (sedra-rusty). `version()` stays the upstream base version, which peers see
/// in the P2P user agent (`kaspad:1.0.1`), so the handshake with Sedra's Go nodes is unchanged.
pub fn sedra_release() -> &'static str {
    "0.1.0"
}

/// SEDRA: what `--version` and the startup log show, e.g. `0.1.0 (rusty-kaspa v1.0.1, commit 9cb74efc1234)`.
pub fn sedra_version_string(commit: Option<&str>) -> String {
    match commit {
        Some(commit) => format!("{} (rusty-kaspa v{}, commit {commit})", sedra_release(), version()),
        None => format!("{} (rusty-kaspa v{})", sedra_release(), version()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sedra_version_string_names_release_base_and_commit() {
        assert_eq!(sedra_version_string(Some("9cb74efc1234")), "0.1.0 (rusty-kaspa v1.0.1, commit 9cb74efc1234)");
        assert_eq!(sedra_version_string(None), "0.1.0 (rusty-kaspa v1.0.1)");
        // the user agent peers see stays the upstream version
        assert_eq!(version(), "1.0.1");
    }
}

pub fn name() -> &'static str {
    "kaspad"
}
