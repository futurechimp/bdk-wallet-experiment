use std::{collections::BTreeMap, str::FromStr};

use bdk_esplora::esplora_client::FromHex;
#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{
    bitcoin::{self, sighash::SighashCache},
    miniscript::{psbt::PsbtExt, Descriptor},
};
use bdk_wallet::{
    bitcoin::{
        psbt, secp256k1, transaction, Address, OutPoint, PrivateKey, Psbt, Script, Sequence,
        Transaction, TxIn, TxOut,
    },
    miniscript::psbt::PsbtInputExt,
};

// These constants can be adjusted to control program flow as desired.
const _AFTER: u32 = 5; // ~2 minutes when using mutinynet.com
const BRIDGE_FUNDS: bool = true; // change this to true when you want to fund the vault
const UNVAULT: bool = true; // switch between unvault and emergency usage

// You shouldn't really need to touch these.
const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;
const ESPLORA_URL: &str = "https://mutinynet.com/api";

mod esplora;
mod keys;
mod utils;

// A port of the vault idea from https://bitcoinerlab.com/guides/miniscript-vault to Rust. Assume we have two users:
//
// * Alice worries a lot about having her keys stolen. She can "unvault" her funds only after the expiration of a timelock.
// * Bob is a Buddhist saint living on a mountain top in Tibet. He can move the locked funds at any time when he gets a phone call from Alice.
//
// We want to use existing keys.
//
// Alice plays the role of the `unvault_key` user. She will keep her funds in the vault. She can't unlock funds until vault time expires.
//
// Alice's good buddy Bob, on the other hand, will be the incorruptible and well-protected Tibetan monk who holds the `emergency_key`.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    utils::tracing_setup();

    // We  have two identities, `alice` and `bob`. Let's assume they have wallets that they're already using, rather than
    // hard-coding policy strings.

    let mut _alice = esplora::Client::new(
        "alice",
        "property blush sun knock heavy animal lens syrup matrix february lava chalk",
        NETWORK,
    )?;
    let _bob = esplora::Client::new(
        "bob",
        "shuffle security crazy source shaft nerve improve bone estate grit brain gold",
        NETWORK,
    )?;

    // alice.sync().await?;
    // alice.balance();

    let secp256k1 = secp256k1::Secp256k1::new();

    let s = "wsh(t:or_c(pk(027a3565454fe1b749bccaef22aff72843a9c3efefd7b16ac54537a0c23f0ec0de),v:thresh(1,pkh(032d672a1a91cc39d154d366cd231983661b0785c7f27bc338447565844f4a6813),a:pkh(03417129311ed34c242c012cd0a3e0b9bca0065f742d0dfb63c78083ea6a02d4d9),a:pkh(025a687659658baeabdfc415164528065be7bcaade19342241941e556557f01e28))))#7hut9ukn";
    let bridge_descriptor = Descriptor::from_str(s).unwrap();
    // let bridge_descriptor =
    //     Descriptor::<bitcoin::PublicKey>::from_str(&s).expect("parse descriptor string");

    assert!(bridge_descriptor.sanity_check().is_ok());
    println!(
        "Bridge pubkey script: {}",
        bridge_descriptor.script_pubkey()
    );
    println!(
        "Bridge address: {}",
        bridge_descriptor.address(Network::Regtest).unwrap()
    );
    println!(
        "Weight for witness satisfaction cost {}",
        bridge_descriptor.max_weight_to_satisfy().unwrap()
    );

    let master_private_key_str = "cQhdvB3McbBJdx78VSSumqoHQiSXs75qwLptqwxSQBNBMDxafvaw";
    let _master_private_key =
        PrivateKey::from_str(master_private_key_str).expect("Can't create private key");
    println!(
        "Master public key: {}",
        _master_private_key.public_key(&secp256k1)
    );

    let backup1_private_key_str = "cWA34TkfWyHa3d4Vb2jNQvsWJGAHdCTNH73Rht7kAz6vQJcassky";
    let backup1_private =
        PrivateKey::from_str(backup1_private_key_str).expect("Can't create private key");

    println!(
        "Backup1 public key: {}",
        backup1_private.public_key(&secp256k1)
    );

    let backup2_private_key_str = "cPJFWUKk8sdL7pcDKrmNiWUyqgovimmhaaZ8WwsByDaJ45qLREkh";
    let backup2_private =
        PrivateKey::from_str(backup2_private_key_str).expect("Can't create private key");

    println!(
        "Backup2 public key: {}",
        backup2_private.public_key(&secp256k1)
    );

    let backup3_private_key_str = "cT5cH9UVm81W5QAf5KABXb23RKNSMbMzMx85y6R2mF42L94YwKX6";
    let _backup3_private =
        PrivateKey::from_str(backup3_private_key_str).expect("Can't create private key");

    println!(
        "Backup3 public key: {}",
        _backup3_private.public_key(&secp256k1)
    );

    let spend_tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::from_consensus(5000),
        input: vec![],
        output: vec![],
    };

    // Spend one input and spend one output for simplicity.
    let mut psbt = Psbt {
        unsigned_tx: spend_tx,
        unknown: BTreeMap::new(),
        proprietary: BTreeMap::new(),
        xpub: BTreeMap::new(),
        version: 0,
        inputs: vec![],
        outputs: vec![],
    };

    let hex_tx = "020000000001018ff27041f3d738f5f84fd5ee62f1c5b36afebfb15f6da0c9d1382ddd0eaaa23c0000000000feffffff02b3884703010000001600142ca3b4e53f17991582d47b15a053b3201891df5200e1f50500000000220020c0ebf552acd2a6f5dee4e067daaef17b3521e283aeaa44a475278617e3d2238a0247304402207b820860a9d425833f729775880b0ed59dd12b64b9a3d1ab677e27e4d6b370700220576003163f8420fe0b9dc8df726cff22cbc191104a2d4ae4f9dfedb087fcec72012103817e1da42a7701df4db94db8576f0e3605f3ab3701608b7e56f92321e4d8999100000000";
    let depo_tx: Transaction =
        bitcoin::consensus::deserialize(&Vec::<u8>::from_hex(hex_tx).unwrap()).unwrap();

    let receiver = Address::from_str("bcrt1qsdks5za4t6sevaph6tz9ddfjzvhkdkxe9tfrcy")
        .unwrap()
        .assume_checked();

    let amount = 100000000;

    let (outpoint, witness_utxo) = get_vout(&depo_tx, &bridge_descriptor.script_pubkey());

    let txin = TxIn {
        previous_output: outpoint,
        sequence: Sequence::from_height(26),
        ..Default::default()
    };
    psbt.unsigned_tx.input.push(txin);

    psbt.unsigned_tx.output.push(TxOut {
        script_pubkey: receiver.script_pubkey(),
        value: Amount::from_sat(amount / 5 - 500),
    });

    psbt.unsigned_tx.output.push(TxOut {
        script_pubkey: bridge_descriptor.script_pubkey(),
        value: Amount::from_sat(amount * 4 / 5),
    });

    // Generating signatures & witness data

    let mut input = psbt::Input::default();
    input
        .update_with_descriptor_unchecked(&bridge_descriptor)
        .unwrap();

    input.witness_utxo = Some(witness_utxo.clone());
    psbt.inputs.push(input);
    psbt.outputs.push(psbt::Output::default());

    let mut sighash_cache = SighashCache::new(&psbt.unsigned_tx);

    let msg = psbt
        .sighash_msg(0, &mut sighash_cache, None)
        .unwrap()
        .to_secp_msg();

    // Fixme: Take a parameter
    let hash_ty = bitcoin::sighash::EcdsaSighashType::All;

    let sk1 = backup1_private.inner;
    let sk2 = backup2_private.inner;

    // Finally construct the signature and add to psbt
    let sig1 = secp256k1.sign_ecdsa(&msg, &sk1);
    let pk1 = backup1_private.public_key(&secp256k1);
    assert!(secp256k1.verify_ecdsa(&msg, &sig1, &pk1.inner).is_ok());

    // Second key just in case
    let sig2 = secp256k1.sign_ecdsa(&msg, &sk2);
    let pk2 = backup2_private.public_key(&secp256k1);
    assert!(secp256k1.verify_ecdsa(&msg, &sig2, &pk2.inner).is_ok());

    psbt.inputs[0].partial_sigs.insert(
        pk1,
        bitcoin::ecdsa::Signature {
            signature: sig1,
            sighash_type: hash_ty,
        },
    );

    println!("{:#?}", psbt);
    println!("{}", psbt);

    psbt.finalize_mut(&secp256k1).unwrap();
    println!("{:#?}", psbt);

    let tx = psbt.extract_tx().expect("failed to extract tx");
    println!("{}", bitcoin::consensus::encode::serialize_hex(&tx));

    // Fund the vault if needed, using the regular Alice wallet and a simple transfer.
    // This constant is set up at the top of the file
    if BRIDGE_FUNDS {
        // let mut transfer_psbt = alice.simple_transfer(vault_address, Amount::from_sat(500))?;
        // tracing::info!("transfer_psbt: {} ", transfer_psbt);

        // // Sign the transaction
        // let finalized = alice
        //     .wallet
        //     .sign(&mut transfer_psbt, SignOptions::default())?;

        // assert!(finalized);

        // let transaction = transfer_psbt.extract_tx()?;
        // // Broadcast the transaction
        // alice.broadcast(&transaction).await?;
        // tracing::info!(
        //     "transaction is at: https://mutinynet.com/tx/{} ",
        //     transaction.compute_txid()
        // );
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

// Find the Outpoint by spk
fn get_vout(tx: &Transaction, spk: &Script) -> (OutPoint, TxOut) {
    for (i, txout) in tx.clone().output.into_iter().enumerate() {
        if spk == &txout.script_pubkey {
            return (OutPoint::new(tx.compute_txid(), i as u32), txout);
        }
    }
    panic!("Only call get vout on functions which have the expected outpoint");
}
