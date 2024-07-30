use std::path::PathBuf;

pub(crate) const BITCOIN_KEY_FILE: &str = "bitcoin_keys.pem";
pub(crate) const BFT_CRDT_KEY_FILE: &str = "keys.pem";
pub(crate) const CONFIG_FILE: &str = "config.toml";

/// Returns the path to the key file and config for this host OS.
pub(crate) fn side_paths(prefix: PathBuf) -> (PathBuf, PathBuf, PathBuf) {
    let mut bft_crdt_key_path = prefix.clone();
    bft_crdt_key_path.push(BFT_CRDT_KEY_FILE);

    let mut bitcoin_key_path = prefix.clone();
    bitcoin_key_path.push(BITCOIN_KEY_FILE);

    let mut config_path = prefix.clone();
    config_path.push(CONFIG_FILE);

    (bft_crdt_key_path, bitcoin_key_path, config_path)
}

/// Returns the path to the home directory for this host OS and the given node name
pub(crate) fn home(name: &str) -> std::path::PathBuf {
    let mut path = dirs::home_dir().unwrap();
    path.push(".side");
    path.push(name);
    path
}
