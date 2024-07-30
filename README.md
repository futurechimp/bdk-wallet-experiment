# BDK Descriptor Wallet Usage

A quick tryout of the (new-style) BDK wallet, which is the way to use BDK for the future. All old BDK stuff is going to be retired soon.

The project is basically a way to try and figure out the process of generating policies + descriptors, extracting them into PSBTS, signing those cooperatively, putting them into the blockchain, and processing them as (semi-retarded) smart contracts.

Because the BDK docs don't really have a full, complex example like this, I'm going to take the TypeScript project over at [Bitcoiner Labs](https://bitcoinerlab.com/guides/miniscript-vault) ([Github](https://github.com/bitcoinerlab/playground/tree/main/descriptors/miniscript) and recreate it in Rust.


## Understanding the basic flow

The Bitcoiner Labs tutorial makes a very simple *Bitcoin vault*.

Funds are locked into the vault address, and they can't be moved until after a timelock. If the user becomes concerned that maybe somebody has stolen their regular mnemonic, the idea is that they have another "emergency key" in another country (perhaps stored with Grandma) and they can fly there and *immediately* access funds.

This will serve to demo a complex policy, descriptor, and singing process, and there's a working TypeScript example to work from so I can see all the steps.

Breaking things down, the steps are as follows.

First, generate two keypairs:

a) one keypair for "normal" usage, a mnemonic.
b) one emergency keypair

The keys derived from the mnemonic cannot move the funds before expiration of the timelock. The emergency keypair can move funds at any time.

Next, generate a Miniscript Policy, stating spending conditions. It looks like this:

```rust
or(pk({emergency_key}),and(pk({unvault_key}),after({after}))
```

In contrast to Bitcoin Script op_codes, the logic here is admirably easy to see. At the top-level, it's an `or` condition, so there are two spend paths. The second path (after the first comma) has one sub-`AND` condition glued together by another comma.

Either:

1. you present the `emergency_key`, OR
2. you present the `unvault_key` AND the current block time is greater than the `after` parameter.

The parameters are (I guess) public keys. The text is maddeningly unspecific about exactly what types those are, so it'll be a bit of exploration.

1. `emergency_key` is a WIF file which they get from an `ECPair.toWIF()`
1. For `unvault_key`, things devolve into JavaScript underneath the hood of the shiny TypeScript library, but digging a bit it looks like a `PBKDF2-HMAC: RFC 2898 key derivation function` wrapped inside a BIP32 (mnemonic) derivation.
1. The `after` parameter is an absolute block number. They construct by retrieving the current block and then specifying it as a number of blocks past the current block.

Ok, seems doable.

So we have two keys, a block number afte which funds can be unvaulted, and an abstract policy. What happens next?

They do a mutable string replace to sub the `after` parameter into the policy, compile the policy (without the keys in place) and check if the policy is sane (very interesting).

Only after sanity checking the policy do they take the miniscript of the compiled policy and turn it into a `wsh` **descriptor**, subbing in the keys:

```ts
// Replace the @unvaultKey and @emergencyKey placeholders with the actual keys
const wshDescriptor = `wsh(${miniscript
  .replace(
    '@unvaultKey',
    descriptors.keyExpressionBIP32({
      masterNode: unvaultMasterNode,
      originPath: WSH_ORIGIN_PATH,
      keyPath: WSH_KEY_PATH
    })
  )
  .replace('@emergencyKey', emergencyPair.publicKey.toString('hex'))})`;
```

At this point, they've got a `wshDescriptor` string. This is where things depart quite strongly from the Rust equivalent.

They create an `Output` interface, which I guess is a UTXO output:

```ts
// Make a new Output object with the wshDescriptor
const wshOutput = new Output({
  descriptor: wshDescriptor,
  network,
  signersPubKeys: [EMERGENCY_RECOVERY ? emergencyPair.publicKey : unvaultKey]
});
```

They then get the (not-yet-existing) address of the `wshOutput`, and check if the UTXO exists on the blockchain. The first time, of course, it won't exist. So they halt, and ask you to fund the `wshAddress`, which was derived off-chain like so:

```ts
const wshAddress = wshOutput.getAddress();
```

Fund the address with a straight-up faucet transfer (nothing fancy). Once you fund the `wshAddress`, and let Bitcoin catch up, you run the program again. This gets you into the vault code path. It's a little convoluted but there are quite a few moving parts.

Once we're in the funded path, we

1. retrieve the first UTXO id and value of the funding transaction
2. set up a brand new PSBT, locked to the correct network (in their case, testnet). Let's call this the input PSBT.
3. get an `inputFinalizer` from the `wshOutput`, by calling `updatePsbtAsInput`.
4. they construct another `Output` which just shoot vault funds at some hardcoded address. We don't really care where the funds go, this is just to simulate the act of transferring to different addresses based on whether we are using the regular unvault key (in which case funds go to `tb1q4280xax2lt0u5a5s9hd4easuvzalm8v9ege9ge` or the emergency key (in which case funds go to `mkpZhYtJu2r87Js3pDiWJDmPte2NRZ8bJV`). Let's call this second PSBT the output PSBT.
5. they update the input PSBT with the output PSBT:

```ts
new Output({
  descriptor: `addr(${
    EMERGENCY_RECOVERY
      ? 'mkpZhYtJu2r87Js3pDiWJDmPte2NRZ8bJV'
      : 'tb1q4280xax2lt0u5a5s9hd4easuvzalm8v9ege9ge'
  })`,
  network
}).updatePsbtAsOutput({ psbt, value: inputValue - 1000 });
```

To me this looks a lot like psbt updates in Rust, I guess we will find out. The above flow is completely commented in my TypeScript version.

## WTF?

All of this, so far, has happened completely offline: no activity other than the faucet funding has happened on the blockchain. This is spooky: am I funding some construction that was already there on the blockchain? How can the vault work at all if the initial funding transaction was just sent to a normal address? This seems to be the central mystery.

## Success

Lastly, they sign with either unvault key or emergency key, and broadcast the transaction. Depending on which key is used for signing, the transaction may:

1. move immediately to `mkpZhYtJu2r87Js3pDiWJDmPte2NRZ8bJV` if the `emergency_key` was used,
2. fail at the miner level if the `unvault_key` was used but the timelock for `after` has not been reached, or
3. move to `tb1q4280xax2lt0u5a5s9hd4easuvzalm8v9ege9ge` if the `unvault_key` was used `after` the timelock

The question in the heading "WTF?" is now the central mystery, even before plowing through the crypto stuff.