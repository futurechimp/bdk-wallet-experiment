use std::{fs, path::PathBuf};

use bdk_wallet::{
    bip39::Mnemonic,
    bitcoin::{
        bip32::{Xpriv, Xpub},
        key::Secp256k1,
        Network,
    },
    keys::{DerivableKey, ExtendedKey},
};

use crate::NETWORK;

pub(crate) fn load_from_file(side_dir: &PathBuf) -> anyhow::Result<(Xpriv, Xpub)> {
    let mnemonic_path = crate::utils::side_paths(side_dir.clone()).1; // TODO: this tuple stinks
    let mnemonic_words = fs::read_to_string(mnemonic_path).expect("couldn't read bitcoin key file");
    println!("Creating extended key from mnemonic: {mnemonic_words}");
    generate_extended_key(mnemonic_words, NETWORK)
}

/// Creates Bitcoin descriptors from a mnemonic
fn generate_extended_key(
    mnemonic_words: String,
    network: Network,
) -> anyhow::Result<(Xpriv, Xpub)> {
    let mnemonic = Mnemonic::parse(mnemonic_words).unwrap();

    // Generate the extended key
    let xkey: ExtendedKey = mnemonic
        .clone()
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");

    let xpriv = xkey
        .into_xprv(network)
        .expect("unable to turn xkey into xpriv");

    // Generate the public key, this is a bit gross but it's necessary as xkey
    // gets consumed immediately on use.
    let xkey: ExtendedKey = mnemonic
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");
    let secp = Secp256k1::new();
    let xpub: Xpub = xkey
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey")
        .into_xpub(network, &secp);

    Ok((xpriv, xpub))
}
