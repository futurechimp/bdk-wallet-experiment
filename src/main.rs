use bdk_wallet::{
    bitcoin::{absolute::LockTime, EcdsaSighashType},
    miniscript::{psbt::PsbtInputExt, DefiniteDescriptorKey},
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
    bitcoin::{self, bip32::Xpub, key::Secp256k1, sighash::SighashCache},
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

// We will use BDK's Esplora client and the Mutiny Signet chain, which has a
// very convenient block time of 30 seconds and an easy-to-access faucet.
const ESPLORA_URL: &str = "https://mutinynet.com/api";

// A pretty standard sync method, shouldn't be anything controversial here.
pub(crate) async fn sync(
    client: &esplora_client::AsyncClient,
    wallet: &mut Persisted<Wallet>,
    conn: &mut rusqlite::Connection,
) {
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
        .full_scan(request, 5, 5)
        .await
        .expect("full scan problem");
    let now = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
    let _ = update.graph_update.update_last_seen_unconfirmed(now);

    wallet
        .apply_update(update)
        .expect("couldn't apply wallet update");
    wallet.persist(conn).expect("couldn't persist wallet");
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
    let secp = Secp256k1::new();

    // Create a wallet and keys for alice
    //
    let alice_words = "property blush sun knock heavy animal lens syrup matrix february lava chalk";
    let alice_mnemonic = Mnemonic::parse(alice_words).expect("can't parse alice's mnemonic");

    let alice_xkey: ExtendedKey = alice_mnemonic
        .clone()
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");
    let alice_xprv = alice_xkey
        .into_xprv(Network::Signet)
        .expect("unable to turn xkey into xpriv");

    // Re-generate so we can grab the public key, alice_xkey above was consumed on use.
    let alice_xkey: ExtendedKey = alice_mnemonic
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");

    let alice_xpub: Xpub = alice_xkey
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey")
        .into_xpub(Network::Signet, &secp);

    // Create an Esplora client
    let alice_client = esplora_client::Builder::new(ESPLORA_URL)
        .build_async()
        .expect("couldn't build client");

    let mut alice_conn = rusqlite::Connection::open("alice-bdk-wallet.sqlite")
        .expect("couldn't open alice's rusqlite connection");

    let external_descriptor = Bip84(alice_xprv.clone(), KeychainKind::External);
    let internal_descriptor = Bip84(alice_xprv.clone(), KeychainKind::Internal);
    let mut alice_wallet = Wallet::create(external_descriptor, internal_descriptor)
        .network(Network::Signet)
        .create_wallet(&mut alice_conn)
        .expect("couldn't create wallet");

    // Sync Alice's client and wallet
    sync(&alice_client, &mut alice_wallet, &mut alice_conn).await;

    // Create a wallet and keys for bob
    //
    let bob_words =
        "bullet venture draft evidence kitchen transfer rare surround bring left tennis powder";
    let bob_mnemonic = Mnemonic::parse(bob_words).expect("can't parse bob's mnemonic");

    let bob_xkey: ExtendedKey = bob_mnemonic
        .clone()
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");
    let bob_xprv = bob_xkey
        .into_xprv(Network::Signet)
        .expect("unable to turn xkey into xpriv");

    // Re-generate so we can grab the public key, bob_xkey above was consumed on use.
    let bob_xkey: ExtendedKey = bob_mnemonic
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey");

    let bob_xpub: Xpub = bob_xkey
        .into_extended_key()
        .expect("couldn't turn mnemonic into xkey")
        .into_xpub(Network::Signet, &secp);

    // Create an Esplora client for Bob
    let _bob_client = esplora_client::Builder::new(ESPLORA_URL)
        .build_async()
        .expect("couldn't build client");

    let mut bob_conn = rusqlite::Connection::open("bob-bdk-wallet.sqlite")
        .expect("couldn't open bob's rusqlite connection");

    let external_descriptor = Bip84(bob_xprv.clone(), KeychainKind::External);
    let internal_descriptor = Bip84(bob_xprv.clone(), KeychainKind::Internal);
    let _bob_wallet = Wallet::create(external_descriptor, internal_descriptor)
        .network(Network::Signet)
        .create_wallet(&mut bob_conn)
        .expect("couldn't create wallet");

    // Ok, now we have two clients, keys, and wallets. Let's set up a vault policy.

    // Get Alice's public key so we can write it into the policy/descriptor
    let unvault_key = alice_xpub.public_key;
    println!("\nalice's unvault_key: {}", unvault_key);

    // Get Bob's public key so we can write it into the policy/descriptor
    let emergency_key = bob_xpub.public_key;
    println!("\nbob's emergency_key: {}", emergency_key);

    // We don't want our "after" variable to change all the time, hardcode it for the moment.
    // TODO: change this once the vault lock is working, after which the script can be made
    // more dynamic.
    let after = 1311208 + 10000;

    // Format out the vault policy
    let policy_str = format!("or(pk({emergency_key}),and(pk({unvault_key}),after({after})))");
    let policy =
        Concrete::<DefiniteDescriptorKey>::from_str(&policy_str).expect("couldn't create policy");
    let descriptor = Descriptor::new_wsh(policy.compile().expect("policy compilation failed"))
        .expect("could not create descriptor");

    assert!(descriptor.sanity_check().is_ok());
    println!("Policy is: {}", policy);
    println!("Descriptor is: {}", descriptor);
    println!("Descriptors can have an address and their own script_pubkey()");
    println!(
        "Descriptor address: https://mutinynet.com/address/{}",
        descriptor.address(Network::Signet).unwrap()
    );
    println!("Descriptor pubkey script: {:?}", descriptor.script_pubkey());

    // Now that we have an address for the descriptor, we can deposit funds into it.

    // Check whether Alice has enough funds. Anyone can lock funds, we will just use Alice's
    // wallet so we don't need to create yet another one. If alice doesn't have the funds,
    // pause for 60 seconds so that there's a chance to fund the account.
    if alice_wallet.balance().total().lt(&Amount::from_sat(amount)) {
        println!("You don't have any funds to deposit into the descriptor's address.");
        println!("We will wait here for a minute until you hit the Mutinynet faucet");
        println!(
            "Please visit https://faucet.mutinynet.com and send some sats to Alice at {}",
            alice_wallet.next_unused_address(KeychainKind::External)
        );
        sleep(Duration::from_secs(60)).await;
    }

    // Build a deposit transaction to send funds to the descriptor address. These funds can only be unlocked
    // by a spending transaction that can match the conditions that are in the descriptor.
    let mut tx_builder = alice_wallet.build_tx();
    tx_builder
        .add_recipient(descriptor.script_pubkey(), Amount::from_sat(amount))
        .enable_rbf();
    let mut deposit_psbt = tx_builder.finish().expect("couldn't finish deposit_psbt");
    let finalized = alice_wallet
        .sign(&mut deposit_psbt, SignOptions::default())
        .expect("couldn't finalize deposit psbt");
    assert!(finalized);

    // Extract the transaction from the deposit psbt
    let deposit_tx = deposit_psbt
        .extract_tx()
        .expect("couldn't extract deposit tx");

    // Broadcast the deposit transaction
    alice_client
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
        script_pubkey: alice_wallet
            .next_unused_address(KeychainKind::External)
            .script_pubkey(),
        value: Amount::from_sat(amount - 250),
    });

    // Generating signatures & witness data
    let mut input = psbt::Input::default();
    input.update_with_descriptor_unchecked(&descriptor).unwrap();
    input.witness_utxo = Some(witness_utxo.clone());
    psbt.inputs.push(input);
    psbt.outputs.push(psbt::Output::default());

    // Construct our own SighashCache which we can use for signing.
    // TODO: check whether it's possible to use the wallet's internal
    // signing mechanism and get rid of a lot of this manually-constructed
    // signing code.
    let mut sighash_cache = SighashCache::new(&psbt.unsigned_tx);
    let msg = psbt
        .sighash_msg(0, &mut sighash_cache, None)
        .unwrap()
        .to_secp_msg();

    // Sign the message with Alice's private key
    let alice_sig = secp.sign_ecdsa(&msg, &alice_xprv.private_key);

    psbt.inputs[0].partial_sigs.insert(
        alice_xpub.public_key.into(),
        bitcoin::ecdsa::Signature {
            signature: alice_sig,
            sighash_type: EcdsaSighashType::All,
        },
    );

    // Finalize the psbt
    // PROBLEM: flames out with: problem finalizing psbt: [InputError(MiniscriptError(CouldNotSatisfy), 0)]
    psbt.finalize_mut(&secp).expect("problem finalizing psbt");

    // Extract the transaction from the psbt
    let my_spend_tx = psbt.extract_tx().expect("failed to extract tx");

    // Broadcast it to spend! This should fail, because although we are using Alice's
    // key to spend, the timelock has not yet elapsed.
    alice_client
        .broadcast(&my_spend_tx)
        .await
        .expect("problem broadcasting spend_tx");

    println!(
        "spend_tx is at https://mutinynet.com/tx/{}",
        my_spend_tx.compute_txid()
    )
}
