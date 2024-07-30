use std::{collections::BTreeSet, io::Write};

use bdk_esplora::{
    esplora_client::{self, AsyncClient},
    EsploraAsyncExt,
};
use bdk_wallet::{
    bitcoin::{Amount, Network},
    chain::Persisted,
    rusqlite, KeychainKind, SignOptions, Wallet,
};
use tracing::Level;
use tracing_subscriber::{filter, fmt, layer::SubscriberExt, Layer, Registry};

const SEND_AMOUNT: Amount = Amount::from_sat(5000);
const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;
const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;

const EXTERNAL_DESC: &str = "wpkh(tprv8ZgxMBicQKsPdy6LMhUtFHAgpocR8GC6QmwMSFpZs7h6Eziw3SpThFfczTDh5rW2krkqffa11UpX3XkeTTB2FvzZKWXqPY54Y6Rq4AQ5R8L/84'/1'/0'/0/*)";
const INTERNAL_DESC: &str = "wpkh(tprv8ZgxMBicQKsPdy6LMhUtFHAgpocR8GC6QmwMSFpZs7h6Eziw3SpThFfczTDh5rW2krkqffa11UpX3XkeTTB2FvzZKWXqPY54Y6Rq4AQ5R8L/84'/1'/0'/1/*)";
const ESPLORA_URL: &str = "https://mutinynet.com/api";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_setup();
    let client = esplora_client::Builder::new(ESPLORA_URL).build_async()?;
    let mut conn = rusqlite::Connection::open(DB_PATH)?;

    let wallet_opt = Wallet::load()
        .descriptors(EXTERNAL_DESC, INTERNAL_DESC)
        .network(NETWORK)
        .load_wallet(&mut conn)?;

    let mut wallet = match wallet_opt {
        Some(wallet) => wallet,
        None => Wallet::create(EXTERNAL_DESC, INTERNAL_DESC)
            .network(NETWORK)
            .create_wallet(&mut conn)?,
    };

    let address = wallet.next_unused_address(KeychainKind::External);
    wallet.persist(&mut conn)?;
    tracing::info!("Next unused address: ({}) {}", address.index, address);

    let balance = wallet.balance();
    tracing::info!("Wallet balance before syncing: {} sats", balance.total());

    sync(&mut wallet, &mut conn, &client).await?;

    let balance = wallet.balance();
    tracing::info!("Wallet balance after syncing: {} sats", balance.total());

    if balance.total() < SEND_AMOUNT {
        tracing::info!(
            "Please send at least {} sats to the receiving address",
            SEND_AMOUNT
        );
        std::process::exit(0);
    }

    let mut tx_builder = wallet.build_tx();
    tx_builder
        .add_recipient(address.script_pubkey(), SEND_AMOUNT)
        .enable_rbf();

    let mut psbt = tx_builder.finish()?;
    let finalized = wallet.sign(&mut psbt, SignOptions::default())?;
    assert!(finalized);

    let tx = psbt.extract_tx()?;
    client.broadcast(&tx).await?;
    tracing::info!("Tx broadcasted! Txid: {}", tx.compute_txid());

    Ok(())
}

async fn sync(
    wallet: &mut Persisted<Wallet>,
    conn: &mut rusqlite::Connection,
    client: &AsyncClient,
) -> anyhow::Result<()> {
    print!("Syncing...");

    let request = wallet.start_full_scan().inspect_spks_for_all_keychains({
        let mut once = BTreeSet::<KeychainKind>::new();
        move |keychain, spk_i, _| {
            if once.insert(keychain) {
                print!("\nScanning keychain [{:?}] ", keychain);
            }
            print!(" {:<3}", spk_i);
            std::io::stdout().flush().expect("must flush")
        }
    });

    let mut update = client
        .full_scan(request, STOP_GAP, PARALLEL_REQUESTS)
        .await?;
    let now = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
    let _ = update.graph_update.update_last_seen_unconfirmed(now);

    wallet.apply_update(update)?;
    wallet.persist(conn)?;

    Ok(())
}

fn tracing_setup() {
    // show only info level logs and above:
    let info = filter::LevelFilter::from_level(Level::INFO);

    // set up the tracing subscriber:
    let subscriber = Registry::default().with(fmt::layer().with_filter(info));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("Tracing initialized.");
}
