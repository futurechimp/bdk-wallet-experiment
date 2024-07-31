use std::str::FromStr;

#[allow(unused)]
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::{bitcoin::PublicKey, miniscript::policy::Concrete};

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

    // We already have two identities, `dave` and `sammy`. Let's go through the steps in the README to work through the vault:

    // We do need to figure out the `after` parameter at which the vault will expire:
    let current = dave.get_current_block_height().await?;
    //

    // Set up the policy: or(pk({@emergency_key}),and(pk({@unvault_key}),after({after})). No keys needed here.
    let policy_str = "or(pk(@emergency_key),and(pk(@unvault_key),after(@after)))";
    let policy = Concrete::<String>::from_str(&policy_str)?;

    // Create the vault descriptor: `wsh(or(pk({emergency_key}),and(pk({unvault_key}),after({after})))`. Include the keys this time.
    let descriptor = Descriptor::new_wsh(policy.compile()?)?.to_string();

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
