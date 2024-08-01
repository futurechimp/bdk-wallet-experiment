use std::{collections::BTreeMap, str::FromStr};

#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{
    bitcoin::{self, sighash::SighashCache},
    miniscript::{psbt::PsbtExt, Descriptor},
};
use bdk_wallet::{
    bitcoin::{
        psbt, secp256k1, transaction, OutPoint, Psbt, Script, Sequence, Transaction, TxIn, TxOut,
    },
    miniscript::psbt::PsbtInputExt,
};

// These constants can be adjusted to control program flow as desired.
const _AFTER: u32 = 5; // ~2 minutes when using mutinynet.com
const BRIDGE_FUNDS: bool = false; // change this to true when you want to fund the vault
const UNBRIDGE_FUNDS: bool = true; // switch between unvault and emergency usage

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

    // Alice will hold the master private key
    let mut alice = esplora::Client::new(
        "alice",
        "property blush sun knock heavy animal lens syrup matrix february lava chalk",
        NETWORK,
    )?;

    // Bob holds backup1
    let bob = esplora::Client::new(
        "bob",
        "shuffle security crazy source shaft nerve improve bone estate grit brain gold",
        NETWORK,
    )?;

    // Carol holds backup2
    let carol = esplora::Client::new(
        "carol",
        "advance simple kitchen monitor twice rescue crunch you party clerk screen inherit",
        NETWORK,
    )?;

    // Dave holds backup2
    let dave = esplora::Client::new(
        "dave",
        "uniform you loop talk orchard polar issue chronic priority garbage actress cradle",
        NETWORK,
    )?;

    alice.sync().await?;
    alice.balance();

    let secp256k1 = secp256k1::Secp256k1::new();

    let alice_pk = alice.wallet_public_key.to_string();
    let bob_pkh = bob
        .wallet_public_key
        // .to_pubkeyhash(SigType::Ecdsa)
        .to_string();
    let carol_pkh = carol
        .wallet_public_key
        // .to_pubkeyhash(SigType::Ecdsa)
        .to_string();
    let dave_pkh = dave
        .wallet_public_key
        // .to_pubkeyhash(SigType::Ecdsa)
        .to_string();

    println!("alice: {}", alice_pk);
    println!("bob: {}", bob_pkh);

    let s = format!("wsh(t:or_c(pk({alice_pk}),v:thresh(1,pkh({bob_pkh}),a:pkh({carol_pkh}),a:pkh({dave_pkh}))))#0y76yhr7");
    let bridge_descriptor = Descriptor::from_str(&s).unwrap();

    assert!(bridge_descriptor.sanity_check().is_ok());
    println!(
        "Bridge pubkey script: {}",
        bridge_descriptor.script_pubkey()
    );
    println!(
        "Bridge address: {}",
        bridge_descriptor.address(Network::Signet).unwrap()
    );
    println!(
        "Weight for witness satisfaction cost {}",
        bridge_descriptor.max_weight_to_satisfy().unwrap()
    );

    // Transfer some funds into the bridge
    let deposit_psbt = alice.simple_transfer(
        bridge_descriptor.address(Network::Signet)?,
        Amount::from_sat(10000),
    )?;
    let deposit_tx = deposit_psbt.extract_tx()?;

    if BRIDGE_FUNDS {
        alice.broadcast(&deposit_tx).await?;
    }

    println!(
        "depo tx may be at: https://mutinynet.com/tx/{}",
        deposit_tx.compute_txid()
    );

    let spend_tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::from_consensus(50),
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

    let receiver = alice.next_unused_address()?;
    println!("receiver: {}", receiver);

    let amount = 10000;

    let (outpoint, witness_utxo) = get_vout(&deposit_tx, &bridge_descriptor.script_pubkey());

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

    let sk1 = bob.wallet_private_key;
    let sk2 = carol.wallet_private_key;

    // Finally construct the signature and add to psbt
    let sig1 = secp256k1.sign_ecdsa(&msg, &sk1);
    let pk1: bitcoin::PublicKey = sk1.public_key(&secp256k1).into();

    // assert!(secp256k1.verify_ecdsa(&msg, &sig1, &pk1).is_ok());

    // Second key just in case
    let sig2 = secp256k1.sign_ecdsa(&msg, &sk2);
    let pk2 = sk2.public_key(&secp256k1);
    assert!(secp256k1.verify_ecdsa(&msg, &sig2, &pk2).is_ok());

    psbt.inputs[0].partial_sigs.insert(
        pk1,
        bitcoin::ecdsa::Signature {
            signature: sig1,
            sighash_type: hash_ty,
        },
    );

    // println!("{:#?}", psbt);
    // println!("{}", psbt);

    psbt.finalize_mut(&secp256k1).unwrap();
    // println!("finalize: {:#?}", psbt);

    let unbridge_tx = psbt.extract_tx().expect("failed to extract tx");
    // println!("{}", bitcoin::consensus::encode::serialize_hex(&tx));

    // This constant is set up at the top of the file
    if UNBRIDGE_FUNDS {
        alice.broadcast(&unbridge_tx).await?;
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
