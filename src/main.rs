use std::str::FromStr;

#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{
    bitcoin::PublicKey,
    miniscript::{self, policy::Concrete},
    KeychainKind, SignOptions,
};
use regex::Regex;

const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;
const ESPLORA_URL: &str = "https://mutinynet.com/api";

const AFTER: u32 = 5; // 2 minutes when using mutiny
const FUND_THE_VAULT: bool = false;

mod esplora;
mod keys;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    utils::tracing_setup();

    let mut dave = esplora::Client::new("dave")?;
    let sammy = esplora::Client::new("sammy")?;

    dave.get_balance();
    dave.sync().await?;

    // We  have two identities, `dave` and `sammy`. Let's go through the steps in the README to work through the vault:

    // We need to figure out the `after` parameter at which the vault will expire:
    let current = dave.get_height().await?;
    let after = current + AFTER;

    let _secp = bdk_wallet::bitcoin::secp256k1::Secp256k1::new();

    // Dave will be the unvault key, Sammy will be the emergency key.
    let unvault_key_fat = dave
        .wallet
        .public_descriptor(KeychainKind::External)
        .derived_descriptor(&_secp, 0)?
        .to_string();
    tracing::info!("unvault_key_fat: {} ", unvault_key_fat);

    // This is the dirtiest coding thing I have done in several decades. The regex extracts out the public key
    // from all the surrounding descriptor cruft that bdk gives back here. I am almost certainly doing something wrong
    // with bdk. I have asked in their Discord channel what the right approach is here.
    let re = Regex::new(r"(\w{66})").unwrap();
    let unvault_key = re
        .captures(&unvault_key_fat)
        .unwrap()
        .get(0)
        .unwrap()
        .as_str();
    tracing::info!("unvault_key: {} ", unvault_key);

    let emergency_key_fat = sammy
        .wallet
        .public_descriptor(KeychainKind::External)
        .derived_descriptor(&_secp, 0)?
        .to_string();
    tracing::info!("emergency_key_fat: {} ", emergency_key_fat);

    let emergency_key = re
        .captures(&emergency_key_fat)
        .unwrap()
        .get(0)
        .unwrap()
        .as_str();
    tracing::info!("emergency_key: {} ", emergency_key);

    // Set up the policy: or(pk({@emergency_key}),and(pk({@unvault_key}),after({after})).
    let policy_str = format!("or(pk({emergency_key}),and(pk({unvault_key}),after({after})))");
    tracing::info!("policy_str: {} ", policy_str);

    let policy = Concrete::<PublicKey>::from_str(&policy_str)?;
    tracing::info!("policy: {} ", policy); // we never get here

    // Create the vault descriptor: `wsh(or(pk({emergency_key}),and(pk({unvault_key}),after({after})))`.
    let descriptor = miniscript::Descriptor::new_wsh(policy.compile()?)?;
    descriptor
        .sanity_check()
        .expect("failed descriptor sanity check");

    tracing::info!("descriptor: {} ", descriptor);

    // Grab the vault address from the descriptor
    let vault_address = descriptor.address(NETWORK)?;
    tracing::info!("address: {} ", vault_address);

    // Fund the vault if needed, using the regular Dave wallet and a simple transfer
    if FUND_THE_VAULT {
        let mut transfer_psbt = dave.simple_transfer(vault_address, Amount::from_sat(500))?;
        tracing::info!("transfer_psbt: {} ", transfer_psbt);

        // Sign the transaction
        let finalized = dave
            .wallet
            .sign(&mut transfer_psbt, SignOptions::default())?;

        assert!(finalized);

        let transaction = transfer_psbt.extract_tx()?;
        // Broadcast the transaction
        dave.broadcast(&transaction).await?;
        tracing::info!(
            "transaction is at: https://mutinynet.com/tx/{} ",
            transaction.compute_txid()
        );
    }

    // Try waiting for the vault to expire, then transfer out again using Dave's wallet
    // let mut unvault_builder = dave.wallet.build_tx();
    // let key = unvault_builder.add_recipient(
    //     dave.next_unused_address()?.script_pubkey(),
    //     Amount::from_sat(250),
    // );

    // Lastly, go through the process again using Sammy's wallet. He should be able to transfer out of the vault any time he wants.

    Ok(())
}
