use bdk_wallet::{
    bitcoin::{Amount, Network},
    Balance, SignOptions,
};

use tracing::Level;
use tracing_subscriber::{filter, fmt, layer::SubscriberExt, Layer, Registry};

const SEND_AMOUNT: Amount = Amount::from_sat(5000);
const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;

const EXTERNAL_DESC: &str = "wpkh(tprv8ZgxMBicQKsPdy6LMhUtFHAgpocR8GC6QmwMSFpZs7h6Eziw3SpThFfczTDh5rW2krkqffa11UpX3XkeTTB2FvzZKWXqPY54Y6Rq4AQ5R8L/84'/1'/0'/0/*)";
const INTERNAL_DESC: &str = "wpkh(tprv8ZgxMBicQKsPdy6LMhUtFHAgpocR8GC6QmwMSFpZs7h6Eziw3SpThFfczTDh5rW2krkqffa11UpX3XkeTTB2FvzZKWXqPY54Y6Rq4AQ5R8L/84'/1'/0'/1/*)";
const ESPLORA_URL: &str = "https://mutinynet.com/api";

mod esplora;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_setup();

    let mut client = esplora::Client::new()?;
    client.get_balance();
    client.sync().await?;

    let balance = client.get_balance();
    ensure_enough_sats(balance);

    // Get the next unused address from the wallet
    let address = client.next_unused_address()?;

    let mut tx_builder = client.wallet.build_tx();
    tx_builder
        .add_recipient(address.script_pubkey(), SEND_AMOUNT)
        .enable_rbf();
    let mut psbt = tx_builder.finish()?;
    let finalized = client.wallet.sign(&mut psbt, SignOptions::default())?;
    assert!(finalized);

    let tx = psbt.extract_tx()?;
    client.broadcast(&tx).await?;
    tracing::info!("Tx broadcasted! Txid: {}", tx.compute_txid());

    Ok(())
}

/// Exit the program if the wallet balance is not enough to send the amount
fn ensure_enough_sats(balance: Balance) {
    if balance.total() < SEND_AMOUNT {
        println!(
            "Please send at least {} sats to the receiving address. Exiting.",
            SEND_AMOUNT
        );
        std::process::exit(0);
    }
}

/// Set up the tracing subscriber, filtering logs to show only info level logs and above
fn tracing_setup() {
    // show only info level logs and above:
    let info = filter::LevelFilter::from_level(Level::INFO);

    // set up the tracing subscriber:
    let subscriber = Registry::default().with(fmt::layer().with_filter(info));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("Tracing initialized.");
}
