# BDK Descriptor Wallet Usage

A quick tryout of the (new-style) BDK wallet, which is the way to use BDK for the future. All old BDK stuff is going to be retired soon.

The project is basically a way to try and figure out the process of generating policies + descriptors, extracting them into PSBTS, signing those cooperatively, putting them into the blockchain, and processing them.

Because the BDK docs don't really have a full, complex example like this, I'm going to take the TypeScript project over at [Bitcoiner Labs](https://bitcoinerlab.com/guides/miniscript-vault) ([Github](https://github.com/bitcoinerlab/playground/tree/main/descriptors/miniscript)) and recreate it in Rust.


## Understanding the basic flow

The Bitcoiner Labs tutorial makes a very simple *Bitcoin vault*.

Funds are locked into the vault address, and they can't be moved until after a timelock. If the user becomes concerned that maybe somebody has stolen their regular mnemonic, the idea is that they have another "emergency key" in another country (perhaps stored with Grandma) and they can fly there and *immediately* access funds.

This will serve to demo a complex policy, descriptor, and signing process, and there's a working TypeScript example to work from so I can see everything in action. It works pretty well.

Breaking things down, the logical steps are as follows.

First, generate two keypairs:

a) one keypair for "normal" usage, i.e. a mnemonic.
b) one emergency keypair, formatted as a WIF file.

The keys derived from the mnemonic cannot move the funds before expiration of the timelock. The emergency keypair can move funds at any time.

Next, generate a Miniscript Policy, stating spending conditions. It looks like this:

```rust
or(pk({emergency_key}),and(pk({unvault_key}),after({after}))
```

In contrast to Bitcoin Script op_codes, the logic here is admirably easy to see. At the top-level, it's an `or` condition, so there are two spend paths. The second path (after the first comma) has one sub-`AND` condition glued together by another comma.

Either:

1. you present the `emergency_key`, OR
2. you present the `unvault_key` AND the current block time is greater than the `after` parameter.

The key parameters are public keys. The text is maddeningly unspecific about exactly what types those are, so it'll be a bit of exploration to figure out exact types in Rust.

1. `emergency_key` is a WIF file which they get from an `ECPair.toWIF()`
1. For `unvault_key`, things devolve into JavaScript underneath the hood of the shiny TypeScript library, but digging a bit it looks like a `PBKDF2-HMAC: RFC 2898 key derivation function` wrapped inside a BIP32 (mnemonic) derivation.
1. The `after` parameter is an absolute block number. They construct by retrieving the current block and then specifying it as a number of blocks past the current block.

Ok, seems doable. Let's explore the code.

We have two keys, a block number after which funds can be unvaulted, and an abstract policy sitting in a string. What happens next?

They do a mutable string replace to sub the `after` parameter into the policy, compile the policy (without the keys in place), and check if the policy is sane.

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

At this point, they've got a `wshDescriptor` string. This is where I suspect things will depart quite strongly from the Rust equivalent.

They create an `Output` interface, which I guess is a UTXO output:

```ts
// Make a new Output object with the wshDescriptor
const wshOutput = new Output({
  descriptor: wshDescriptor,
  network,
  signersPubKeys: [EMERGENCY_RECOVERY ? emergencyPair.publicKey : unvaultKey]
});
```

They then get the (not-yet-existing-on-chain) address of the `wshOutput`, and check if the UTXO exists on the blockchain. The first time, of course, it won't exist. So they halt, and ask you to fund the `wshAddress`, which was derived off-chain like so:

```ts
const wshAddress = wshOutput.getAddress();
```

Fund the address with a straight-up faucet transfer (nothing fancy). Once you fund the `wshAddress`, and let Bitcoin catch up, you run the program again. This gets you into the vault code path. It's a little convoluted as there are quite a few moving parts.

Once we're in the funded path, we:

1. retrieve the first UTXO id and value of the funding transaction
2. set up a brand new PSBT, locked to the correct network (in their case, testnet). Let's call this the **input PSBT**.
3. get an `inputFinalizer` from the `wshOutput`, by calling `updatePsbtAsInput`.
4. they construct another `Output` which just shoots vault funds at some hardcoded address. We don't really care where the funds go, this is just to simulate the act of transferring to different addresses based on whether we are using the regular unvault key (in which case funds go to `tb1q4280xax2lt0u5a5s9hd4easuvzalm8v9ege9ge` or the emergency key (in which case funds go to `mkpZhYtJu2r87Js3pDiWJDmPte2NRZ8bJV`). Let's call this second PSBT the **output PSBT**.
5. they update the **input PSBT** with the **output PSBT**:

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
To me this looks a lot like psbt updates in Rust, I guess we will find out. The above flow is completely commented in my (local) TypeScript version.

## At this point, you may be wondering, WTF?

Take note: all of this, so far, has happened completely offline. No activity other than the faucet funding has happened on the blockchain. This is spooky: am I funding some construction that was already there on the blockchain? How can the vault work at all if the initial funding transaction was just sent to a normal address? This seems to be the central mystery. Let's solve it below by trying it out.

## Success

Lastly, the user signs with either unvault key or emergency key, and broadcasts the transaction. Depending on which key is used for signing, the transaction may:

1. move immediately to `mkpZhYtJu2r87Js3pDiWJDmPte2NRZ8bJV` if the `emergency_key` was used,
2. fail at the miner level if the `unvault_key` was used but the timelock for `after` has not been reached, or
3. move to `tb1q4280xax2lt0u5a5s9hd4easuvzalm8v9ege9ge` if the `unvault_key` was used `after` the timelock

The question in the heading "WTF?" has now been solved with the magic of descriptors (or at least it seems like magic to me).

The answer to the mystery is actually pretty simple. The `wshAddress` is a just a generated, off-chain bitcoin address (in my most recent experiment, it's `tb1q0u2vw7tauy8zm3k2s7dxe0w0pqxc7kvm84ellggwn66z89tphv6sz8rwuf`). Initially, there is no transaction relating to it on-chain.

We run the script once, and it asks us to send over some funds from the faucet.

Once we do so, the address has funds in it. Here's the important part: the unlocking conditions *of that address* are *the policy constraints of* `wshOutput`. To refresh our memory, that looks like:

```ts
const wshOutput = new Output({
  descriptor: wshDescriptor,
  network,
  signersPubKeys: [EMERGENCY_RECOVERY ? emergencyPair.publicKey : unvaultKey]
});
const wshAddress = wshOutput.getAddress(); // tb1q0u2vw7tauy8zm3k2s7dxe0w0pqxc7kvm84ellggwn66z89tphv6sz8rwuf
```

Note that the policy descriptor `wshDescriptor` is itself embedded inside `wshOutput`. That descriptor contains the conditions:

```
or(pk({emergency_key}),and(pk({unvault_key}),after({after}))
```

with all the values concretely filled in with the proper keys. `signersPubKeys` in the `wshOutput` interface then choose a path *through* the descriptor, by signing using one of the two possible keys: `unvault_key` (subject to timelock) or `emergency_key` (for immediate funds transfer).

Almost all of the work happens offline.

Once funds are moved into the descriptor's vault address (in my case, `tb1q0u2vw7tauy8zm3k2s7dxe0w0pqxc7kvm84ellggwn66z89tphv6sz8rwuf`), they can only be moved subject to the policy. Pretty cool!

## Doing it in Rust

Let's do something similar to the vault idea in Rust. Assume we have two users:

* Alice worries a lot about having her keys stolen.
* Bob is a Buddhist saint living on a mountain top in Tibet

(Basically, we don't care here very much about key generation etc and we've already got the code for that working well in Rust).

So, Alice plays the role of the `unvault_key` user. She will keep her funds in the vault. She can't unlock funds until vault time expires.

Alice's good buddy Bob, on the other hand, will be the incorruptible and well-protected Tibetan monk who holds the `emergency_key`.
