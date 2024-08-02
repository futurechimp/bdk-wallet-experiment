use bdk_wallet::{
    bitcoin::{absolute::LockTime, EcdsaSighashType},
    miniscript::psbt::PsbtInputExt,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    str::FromStr,
    time::Duration,
};
use tokio::time::sleep;

use bdk_esplora::{esplora_client, EsploraAsyncExt};
#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{
    bip39::Mnemonic,
    bitcoin::{self, bip32::Xpub, key::Secp256k1, sighash::SighashCache, PublicKey},
    chain::Persisted,
    keys::{DerivableKey, ExtendedKey},
    miniscript::{psbt::PsbtExt, Descriptor},
    rusqlite,
    template::Bip84,
    KeychainKind, SignOptions, Wallet,
};
use bdk_wallet::{
    bitcoin::{psbt, transaction, OutPoint, Psbt, Script, Transaction, TxIn, TxOut},
    miniscript::policy::Concrete,
};

const DB_PATH: &str = "bdk-wallet.sqlite";
const ESPLORA_URL: &str = "https://mutinynet.com/api";
const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;

// A pretty standard sync method, shouldn't be anything controversial here.
pub(crate) async fn sync(
    client: &esplora_client::AsyncClient,
    wallet: &mut Persisted<Wallet>,
    conn: &mut rusqlite::Connection,
) -> anyhow::Result<()> {
    println!("Syncing...");

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

// Find the OutPoint by spk, useful for ensuring that we grab the right
// output transaction to use as input for our spend transaction
fn get_vout(tx: &Transaction, spk: &Script) -> (OutPoint, TxOut) {
    for (i, txout) in tx.clone().output.into_iter().enumerate() {
        if spk == &txout.script_pubkey {
            return (OutPoint::new(tx.compute_txid(), i as u32), txout);
        }
    }
    panic!("Only call get vout on functions which have the expected outpoint");
}

#[tokio::main]
async fn main() {
    let amount = 1000;

    // create a wallet
    let words = "property blush sun knock heavy animal lens syrup matrix february lava chalk";
    let mnemonic = Mnemonic::parse(words).unwrap();
    let xkey: ExtendedKey = mnemonic
        .clone()
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");
    let xprv = xkey
        .into_xprv(Network::Signet)
        .expect("unable to turn xkey into xpriv");

    // Re-generate so we can grab the public key, xkey above was consumed on use.
    let xkey: ExtendedKey = mnemonic
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");

    let secp = Secp256k1::new();
    let xpub: Xpub = xkey
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey")
        .into_xpub(Network::Signet, &secp);

    // Create an Esplora client
    let client = esplora_client::Builder::new(ESPLORA_URL)
        .build_async()
        .expect("couldn't build client");
    let mut conn = rusqlite::Connection::open(DB_PATH).expect("couldn't open rusqlite connection");

    let external_descriptor = Bip84(xprv.clone(), KeychainKind::External);
    let internal_descriptor = Bip84(xprv.clone(), KeychainKind::Internal);
    let mut wallet = Wallet::create(external_descriptor, internal_descriptor)
        .network(Network::Signet)
        .create_wallet(&mut conn)
        .expect("couldn't create wallet");

    // Sync the client and wallet
    sync(&client, &mut wallet, &mut conn)
        .await
        .expect("client sync failed");

    // Get Alice's public key so we can write it into the policy/descriptor
    let alice_pk = xpub.public_key;
    println!("alice: {}", alice_pk);

    let policy_str = format!("pk({alice_pk})");
    let policy = Concrete::<PublicKey>::from_str(&policy_str).expect("couldn't create policy");
    println!("{}", policy);
    let descriptor =
        Descriptor::new_wsh(policy.compile().unwrap()).expect("could not create descriptor");

    assert!(descriptor.sanity_check().is_ok());
    println!("Descriptor pubkey script: {:?}", descriptor.script_pubkey());
    println!(
        "Descriptor address: {}",
        descriptor.address(Network::Signet).unwrap()
    );

    let mut tx_builder = wallet.build_tx();
    tx_builder
        .add_recipient(descriptor.script_pubkey(), Amount::from_sat(amount))
        .enable_rbf();
    let mut deposit_psbt = tx_builder.finish().expect("couldn't finish deposit_psbt");
    let finalized = wallet
        .sign(&mut deposit_psbt, SignOptions::default())
        .expect("couldn't finalize deposit psbt");
    assert!(finalized);

    // Extract the transaction from the deposit psbt
    let deposit_tx = deposit_psbt
        .extract_tx()
        .expect("couldn't extract deposit tx");

    // Broadcast the transaction
    client
        .broadcast(&deposit_tx)
        .await
        .expect("problem broadcasting deposit tx");

    println!(
        "depo tx is at: https://mutinynet.com/tx/{}",
        deposit_tx.compute_txid()
    );

    // Sleep for 30 seconds to ensure Mutiny mines a block - it has a 30 second block time.
    println!("Sleeping for 30 seconds while we wait for our deposit transaction to be mined");
    sleep(Duration::from_secs(30)).await;

    // At this point, we've got a very simple policy `pk({alice_pk})`, into which we've inserted
    // Alice's public key, turned into a descriptor, and generated an address. We have sent sats
    // into the generated address. How do we transfer the sats back out?
    //
    // My current, perhaps wrong, understanding, is that we need to manually generate a
    // bitcoin::Transaction with UTXOs based on the hex of the deposit_tx transaction
    // we just sent.

    // The only place I have seen code which attempts to spend locked funds is at
    // https://github.com/rust-bitcoin/rust-miniscript/blob/master/examples/psbt_sign_finalize.rs
    // so the code below is largely copied from there. I have tried to boil it down to the simplest
    // possible descriptor.

    let spend_tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };

    let mut psbt = Psbt {
        unsigned_tx: spend_tx,
        unknown: BTreeMap::new(),
        proprietary: BTreeMap::new(),
        xpub: BTreeMap::new(),
        version: 0,
        inputs: vec![],
        outputs: vec![],
    };

    let (outpoint, witness_utxo) = get_vout(&deposit_tx, &descriptor.script_pubkey());

    let txin = TxIn {
        previous_output: outpoint,
        ..Default::default()
    };
    psbt.unsigned_tx.input.push(txin);

    psbt.unsigned_tx.output.push(TxOut {
        script_pubkey: wallet
            .next_unused_address(KeychainKind::External)
            .script_pubkey(),
        value: Amount::from_sat(amount - 250),
    });

    // Generating signatures & witness data
    let mut input = psbt::Input::default();

    // Killer problem: uncommenting `input.update_with_descriptor_unchecked()` causes a compile-time error.
    // The psbt input wants a Descriptor<DefiniteDescriptorKey> rather than
    // a Descriptor<PublicKey>. However switching descriptor types gets other errors I'm not sure
    // how to code around.
    // input.update_with_descriptor_unchecked(&descriptor).unwrap();

    input.witness_utxo = Some(witness_utxo.clone());
    psbt.inputs.push(input);
    psbt.outputs.push(psbt::Output::default());

    let mut sighash_cache = SighashCache::new(&psbt.unsigned_tx);

    // Killer problem: this blows up at runtime with `MissingWitnessScript`,
    // presumably because the input was not updated with the descriptor
    // (see above).
    let msg = psbt
        .sighash_msg(0, &mut sighash_cache, None)
        .unwrap()
        .to_secp_msg();

    // Sign with Alice's private key
    let alice_sig = secp.sign_ecdsa(&msg, &xprv.private_key);

    psbt.inputs[0].partial_sigs.insert(
        xpub.public_key.into(),
        bitcoin::ecdsa::Signature {
            signature: alice_sig,
            sighash_type: EcdsaSighashType::All,
        },
    );

    // println!("{:#?}", psbt);
    // println!("{}", psbt);

    psbt.finalize_mut(&secp).unwrap();

    // println!("finalize: {:#?}", psbt);

    let spend_tx = psbt.extract_tx().expect("failed to extract tx");
    // println!("{}", bitcoin::consensus::encode::serialize_hex(&spend_tx));

    client
        .broadcast(&spend_tx)
        .await
        .expect("problem broadcasting spend_tx");
}
