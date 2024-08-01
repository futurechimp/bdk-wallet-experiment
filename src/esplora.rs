use crate::{keys, DB_PATH, ESPLORA_URL, NETWORK};

use bdk_esplora::{esplora_client, EsploraAsyncExt};
use bdk_wallet::bitcoin::secp256k1::{PublicKey, SecretKey};
use bdk_wallet::bitcoin::{Address, Amount, Network, Psbt, Transaction};
use bdk_wallet::chain::Persisted;
use bdk_wallet::template::Bip84;
use bdk_wallet::{rusqlite, Balance, KeychainKind, SignOptions, Wallet};
use std::collections::BTreeSet;
use std::io::Write;

const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;

pub(crate) struct Client {
    client: esplora_client::AsyncClient,
    conn: rusqlite::Connection,
    name: String,
    pub(crate) wallet_public_key: PublicKey,
    pub(crate) wallet_private_key: SecretKey,
    pub(crate) wallet: Persisted<Wallet>,
}

impl Client {
    /// Create a new esplora::Client instance
    pub(crate) fn new(name: &str, mnemonic: &str, network: Network) -> anyhow::Result<Client> {
        let client = esplora_client::Builder::new(ESPLORA_URL).build_async()?;
        let mut conn = rusqlite::Connection::open(DB_PATH)?;

        let (xprv, xpub) = keys::create_from(mnemonic.to_string(), network)?;

        let external_descriptor = Bip84(xprv.clone(), KeychainKind::External);
        let internal_descriptor = Bip84(xprv.clone(), KeychainKind::Internal);
        let wallet = Wallet::create(external_descriptor, internal_descriptor)
            .network(NETWORK)
            .create_wallet(&mut conn)?; //xkey.into_xpub(NETWORK, wallet.secp_ctx());

        Ok(Client {
            client,
            conn,
            name: name.to_string(),
            wallet_public_key: xpub.public_key,
            wallet_private_key: xprv.private_key,
            wallet,
        })
    }

    #[allow(unused)]
    // Transfer `amount` to `receiver` using this client's wallet
    pub(crate) fn simple_transfer(
        &mut self,
        receiver: Address,
        amount: Amount,
    ) -> anyhow::Result<Psbt> {
        // Sanity check the transfer first
        self.ensure_enough_sats(amount);
        tracing::info!(
            "{}: simple transfer of {} sats to {}",
            self.name,
            amount,
            receiver
        );

        let mut tx_builder = self.wallet.build_tx();
        tx_builder
            .add_recipient(receiver.script_pubkey(), amount)
            .enable_rbf();
        let mut psbt = tx_builder.finish()?;
        let finalized = self.wallet.sign(&mut psbt, SignOptions::default())?;
        assert!(finalized);
        Ok(psbt)
    }

    #[allow(unused)]
    pub(crate) async fn broadcast(&self, transaction: &Transaction) -> anyhow::Result<()> {
        tracing::info!("Broadcasting transaction: {:?}", transaction);
        self.client.broadcast(transaction).await?;
        Ok(())
    }

    #[allow(unused)]
    pub(crate) fn next_unused_address(&mut self) -> anyhow::Result<Address> {
        let address = self.wallet.next_unused_address(KeychainKind::External);
        self.wallet.persist(&mut self.conn)?;
        tracing::info!("Next unused address: ({}) {}", address.index, address);
        Ok(address.address)
    }

    /// Get the wallet balance
    pub(crate) fn balance(&self) -> Balance {
        let balance = self.wallet.balance();
        tracing::info!("Wallet balance: {} sats", balance.total());
        balance
    }

    pub(crate) async fn get_height(&self) -> anyhow::Result<u32> {
        let blockheight = self.client.get_height().await?;
        tracing::info!("Current blockheight: {}", blockheight);
        Ok(blockheight)
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

    /// Exit the program if the wallet balance is not enough to send the amount
    fn ensure_enough_sats(&self, amount: Amount) {
        if self.balance().total() < amount {
            println!(
                "Please send at least {} sats to the receiving address. Exiting.",
                amount
            );
            std::process::exit(0);
        }
    }
}
