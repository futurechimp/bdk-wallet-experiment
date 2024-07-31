use std::str::FromStr;

#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{
    bitcoin::amount::serde::as_sat::deserialize,
    descriptor::IntoWalletDescriptor,
    miniscript::{self, policy::Concrete},
    KeychainKind,
};
use regex::Regex;

const DB_PATH: &str = "bdk-wallet.sqlite";
const NETWORK: Network = Network::Signet;
const ESPLORA_URL: &str = "https://mutinynet.com/api";

const AFTER: u32 = 5; // 2 minutes when using mutiny

mod esplora;
mod keys;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    utils::tracing_setup();

    let mut dave = esplora::Client::new("dave")?;
    let sammy = esplora::Client::new("sammy")?;

    dave.get_balance();
    dave.sync().await?;

    // We  have two identities, `dave` and `sammy`. Let's go through the steps in the README to work through the vault:

    // We need to figure out the `after` parameter at which the vault will expire:
    let current = dave.get_height().await?;
    let after = current + AFTER;

    let _secp = bdk_wallet::bitcoin::secp256k1::Secp256k1::new();

    // Dave will be the unvault key, Sammy will be the emergency key.
    let unvault_key_fat = dave
        .wallet
        .public_descriptor(KeychainKind::External)
        .derived_descriptor(&_secp, 0)?
        .to_string();
    tracing::info!("unvault_key_fat: {} ", unvault_key_fat);

    // This is the dirtiest coding thing I have done in several decades
    let re = Regex::new(r"(\w{66})").unwrap();
    let unvault_key = re
        .captures(&unvault_key_fat)
        .unwrap()
        .get(0)
        .unwrap()
        .as_str();
    tracing::info!("unvault_key: {} ", unvault_key);

    let emergency_key_fat = sammy
        .wallet
        .public_descriptor(KeychainKind::External)
        .derived_descriptor(&_secp, 0)?
        .to_string();
    tracing::info!("emergency_key_fat: {} ", emergency_key_fat);

    let emergency_key = re
        .captures(&emergency_key_fat)
        .unwrap()
        .get(0)
        .unwrap()
        .as_str();
    tracing::info!("emergency_key: {} ", emergency_key);

    // Set up the policy: or(pk({@emergency_key}),and(pk({@unvault_key}),after({after})).
    let policy_str = format!("or(pk({emergency_key}),and(pk({unvault_key}),after({after})))");
    tracing::info!("policy_str: {} ", policy_str);
    // policy_str: or(pk(wpkh(02cd27efdddce3d6ca228f022696c1bc6c7590cd348fdf8439137ef2445f6ecd70)#3w7wrp44),and(pk(wpkh(02c52472014a9e735a8106442d8dc4866bf5206dfb2d49076ba5f00a4482e58f37)#q49am5ge),after(1303374)))

    // TODO: It's dying here. Policy parsing is not happy, I'm not sure whether it's because the above policy string is wrong, but probably. Could it be that the `pk(wpkh(blah)#1234),` stuff is a problem? I don't know what combination of those is in fact allowed in a policy, will have to check.
    let policy = Concrete::<String>::from_str(&policy_str)?;
    tracing::info!("policy: {} ", policy); // we never get here

    // Create the vault descriptor: `wsh(or(pk({emergency_key}),and(pk({unvault_key}),after({after})))`. Include the keys this time.
    let descriptor = miniscript::Descriptor::new_wsh(policy.compile()?)?;
    descriptor
        .sanity_check()
        .expect("failed descriptor sanity check");

    // Somehow create an equivalent of the Output from the TypeScript code. Ensure that the signer keys are set to the Dave and Sammy keys, equivalent to:
    // ```
    // const wshOutput = new Output({
    //   descriptor: wshDescriptor,
    //   network,
    //   signersPubKeys: [EMERGENCY_RECOVERY ? emergencyPair.publicKey : unvaultKey]
    // });
    //```

    // Get the vault address from the Output and print it, equivalent to:
    // ```ts
    // const wshAddress = wshOutput.getAddress();
    // ```

    // Fund the vault if needed, using the regular Dave wallet and a simple transfer

    // Test the vault by attempting to transfer out, using Dave's wallet and a simple transfer

    // Try waiting for the vault to expire, then transfer out again using Dave's wallet

    // Lastly, go through the process again using Sammy's wallet. He should be able to transfer out of the vault any time he wants.

    Ok(())
}
