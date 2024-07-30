use std::{fs, path::PathBuf};

use bdk_wallet::{
    bip39::Mnemonic,
    keys::{DerivableKey, ExtendedKey},
};

pub(crate) fn load_from_file(side_dir: &PathBuf) -> anyhow::Result<ExtendedKey> {
    let mnemonic_path = crate::utils::side_paths(side_dir.clone()).1; // TODO: this tuple stinks
    let mnemonic_words = fs::read_to_string(mnemonic_path).expect("couldn't read bitcoin key file");
    println!("Creating extended key from mnemonic: {mnemonic_words}");
    generate_extended_key(mnemonic_words)
}

/// Creates Bitcoin descriptors from a mnemonic
fn generate_extended_key(mnemonic_words: String) -> anyhow::Result<ExtendedKey> {
    let mnemonic = Mnemonic::parse(mnemonic_words).unwrap();

    // Generate the extended key
    let xkey: ExtendedKey = mnemonic
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");

    Ok(xkey)
}
