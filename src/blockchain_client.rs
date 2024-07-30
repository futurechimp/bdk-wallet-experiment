use crate::{DB_PATH, ESPLORA_URL, EXTERNAL_DESC, INTERNAL_DESC, NETWORK};

use bdk_esplora::{esplora_client, EsploraAsyncExt};
use bdk_wallet::bitcoin::{Address, Transaction};
use bdk_wallet::chain::Persisted;
use bdk_wallet::{rusqlite, Balance, KeychainKind, Wallet};
use std::collections::BTreeSet;
use std::io::Write;

const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;

pub(crate) struct BlockchainClient {
    conn: rusqlite::Connection,
    pub(crate) wallet: Persisted<Wallet>,
    client: esplora_client::AsyncClient,
}

impl BlockchainClient {
    pub(crate) fn new() -> anyhow::Result<BlockchainClient> {
        let client = esplora_client::Builder::new(ESPLORA_URL).build_async()?;
        let mut conn = rusqlite::Connection::open(DB_PATH)?;
        let wallet_opt = Wallet::load()
            .descriptors(EXTERNAL_DESC, INTERNAL_DESC)
            .network(NETWORK)
            .load_wallet(&mut conn)?;

        let wallet = match wallet_opt {
            Some(wallet) => wallet,
            None => Wallet::create(EXTERNAL_DESC, INTERNAL_DESC)
                .network(NETWORK)
                .create_wallet(&mut conn)?,
        };

        Ok(BlockchainClient {
            conn,
            wallet,
            client,
        })
    }

    pub(crate) async fn broadcast(&self, transaction: &Transaction) -> anyhow::Result<()> {
        tracing::info!("Broadcasting transaction: {:?}", transaction);
        self.client.broadcast(transaction).await?;
        Ok(())
    }

    pub(crate) fn next_unused_address(&mut self) -> anyhow::Result<Address> {
        let address = self.wallet.next_unused_address(KeychainKind::External);
        self.wallet.persist(&mut self.conn)?;
        tracing::info!("Next unused address: ({}) {}", address.index, address);
        Ok(address.address)
    }

    /// Get the wallet balance
    pub(crate) fn get_balance(&self) -> Balance {
        let balance = self.wallet.balance();
        tracing::info!("Wallet balance: {} sats", balance.total());
        balance
    }

    /// Sync the local wallet database with the remote Esplora server
    pub(crate) async fn sync(&mut self) -> anyhow::Result<()> {
        tracing::info!("Syncing...");

        let request = self
            .wallet
            .start_full_scan()
            .inspect_spks_for_all_keychains({
                let mut once = BTreeSet::<KeychainKind>::new();
                move |keychain, spk_i, _| {
                    if once.insert(keychain) {
                        print!("\nScanning keychain [{:?}] ", keychain);
                    }
                    print!(" {:<3}", spk_i);
                    std::io::stdout().flush().expect("must flush")
                }
            });

        let mut update = self
            .client
            .full_scan(request, STOP_GAP, PARALLEL_REQUESTS)
            .await?;
        let now = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
        let _ = update.graph_update.update_last_seen_unconfirmed(now);

        self.wallet.apply_update(update)?;
        self.wallet.persist(&mut self.conn)?;

        Ok(())
    }
}
