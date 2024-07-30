use bdk_wallet::bitcoin::{Amount, Network};

const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;
const ESPLORA_URL: &str = "https://mutinynet.com/api";

mod esplora;
mod keys;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    utils::tracing_setup();

    let mut dave = esplora::Client::new("dave")?;
    let mut sammy = esplora::Client::new("sammy")?;

    dave.get_balance();
    dave.sync().await?;

    // Create a PSBT with the amount and the address
    let psbt = dave.simple_transfer(sammy.next_unused_address()?, Amount::from_sat(500))?;

    // Broadcast the transaction to send the funds
    let tx = psbt.extract_tx()?;
    dave.broadcast(&tx).await?;
    tracing::info!(
        "Tx broadcasted! See  https://mutinynet.com/tx/{}",
        tx.compute_txid()
    );

    Ok(())
}
