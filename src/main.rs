use std::str::FromStr;

#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{
    bitcoin::PublicKey,
    miniscript::{self, policy::Concrete},
    SignOptions,
};

// These constants can be adjusted to control program flow as desired.
const AFTER: u32 = 5; // ~2 minutes when using mutinynet.com
const FUND_THE_VAULT: bool = false;
const UNVAULT: bool = true;

// You shouldn't really need to touch these.
const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;
const ESPLORA_URL: &str = "https://mutinynet.com/api";

mod esplora;
mod keys;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    utils::tracing_setup();

    let mut alice = esplora::Client::new(
        "alice",
        "property blush sun knock heavy animal lens syrup matrix february lava chalk",
        NETWORK,
    )?;
    let bob = esplora::Client::new(
        "bob",
        "shuffle security crazy source shaft nerve improve bone estate grit brain gold",
        NETWORK,
    )?;

    alice.balance();
    alice.sync().await?;

    // We  have two identities, `alice` and `bob`.

    // We need to figure out the `after` parameter at which the vault will expire:
    let current = alice.get_height().await?;
    let after = current + AFTER;

    // Alice will be the unvault key
    let unvault_key = alice.wallet_public_key;

    // Bob will be the emergency key.
    let emergency_key = bob.wallet_public_key;

    // Set up the policy: or(pk({@emergency_key}),and(pk({@unvault_key}),after({after})).
    let policy_str = format!("or(pk({emergency_key}),and(pk({unvault_key}),after({after})))");
    tracing::info!("policy_str: {} ", policy_str);

    let policy = Concrete::<PublicKey>::from_str(&policy_str)?;
    tracing::info!("policy: {} ", policy); // we never get here

    // Create the vault descriptor: `wsh(or(pk({emergency_key}),and(pk({unvault_key}),after({after})))`.
    let vault_descriptor = miniscript::Descriptor::new_wsh(policy.compile()?)?;
    vault_descriptor
        .sanity_check()
        .expect("failed descriptor sanity check");

    tracing::info!("descriptor: {} ", vault_descriptor);

    // Grab the vault address from the descriptor
    let vault_address = vault_descriptor.address(NETWORK)?;
    tracing::info!("address: {} ", vault_address);

    // Fund the vault if needed, using the regular Alice wallet and a simple transfer.
    // This constant is set up at the top of the file
    if FUND_THE_VAULT {
        let mut transfer_psbt = alice.simple_transfer(vault_address, Amount::from_sat(500))?;
        tracing::info!("transfer_psbt: {} ", transfer_psbt);

        // Sign the transaction
        let finalized = alice
            .wallet
            .sign(&mut transfer_psbt, SignOptions::default())?;

        assert!(finalized);

        let transaction = transfer_psbt.extract_tx()?;
        // Broadcast the transaction
        alice.broadcast(&transaction).await?;
        tracing::info!(
            "transaction is at: https://mutinynet.com/tx/{} ",
            transaction.compute_txid()
        );
    }

    // This constant is set up at the top of the file
    if UNVAULT {
        // Alice is going to try to transfer the coins out using the unvault key.
        // This will fail if the timelock has not yet expired.
        // I assume I need to create a wallet here using the `vault_descriptor`?
    } else {
        // Bob is going to try and immediately use his emergency key and transfer
        // I assume I need to create a wallet here using the `vault_descriptor`?
    }

    Ok(())
}
