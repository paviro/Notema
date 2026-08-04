# Encryption

End-to-end and device-based, built on [age](https://age-encryption.org). Every
device that can read the journal has its own keypair, and entries are encrypted to
all trusted devices. Adding a device is an explicit approval step performed from a
device that can already read the journal.

The synced folder carries only ciphertext and public key material. Each device's
**private key stays local** and is never written to that folder, so whoever
syncs (Dropbox, iCloud, a Syncthing peer, etc.) only ever sees
ciphertext and cannot read your entries. On-disk key and roster formats are in
[`docs/STORAGE-FORMAT.md`](STORAGE-FORMAT.md#encryption-files).

## What it does and doesn't protect

A serverless, single-owner design, deliberately scoped:

- **Protects:** entry and attachment *contents* are unreadable without a trusted
  device key. The device roster is signed so nobody can add a rogue device
  without the approval flow below.
- **Does not protect:** entries and attachments are **not signed**. Someone with
  **write** access to your synced folder can inject or replace them undetected.
  (They still can't *read* anything, and could equally just delete files.) The
  guarantee is *secrecy*, not *authenticity* against a tamperer who controls the
  sync medium.

Writing an entry only needs the trusted devices' public keys, so `notema log` is
able to add entries without unlocking the private key — you enter your passphrase
only to *read*. Signing every write would mean unlocking the key each time, which
is the tradeoff this design deliberately avoids.

## Enable encryption

On the first device:

```bash
notema encryption enable            # name the device, optionally set a passphrase
```

This creates this device's key, records it as the first (genesis) entry in the
signed roster, and encrypts every existing plaintext entry. You'll be asked
whether to protect the key with a passphrase; pass `--no-passphrase` to skip that
prompt.

> [!IMPORTANT]
> **Back up your key.** It is the only thing that can decrypt this device's view
> of the journal. If you lose every trusted device's key, encrypted entries are
> unrecoverable.
> ```bash
> notema encryption key export ~/notema-key-backup.toml
> ```
> That writes a standalone copy, mode 0600, still passphrase-protected if the key
> is. Restoring it is copying the file back into a config directory as
> `identity.toml`. Copying `~/.config/notema/identity.toml` works too — but only
> while the key is stored there; see [Where your key lives](#where-your-key-lives).

## Add a new device

Adding a device is a request-and-approve handshake that passes files through the
synced folder (including `.age/`). Each step below only works once the previous
step has synced across, so **let sync finish before moving on**.

1. **On the new device — request access:** on first launch, set the journal root
   to your synced folder when the setup assistant asks for the path. Then:
   ```bash
   notema encryption device enroll     # prompts for a device name and optional passphrase
   ```
   This drops a `pending-<id>.toml` request into `.age/` and prints the device's
   **fingerprint** — keep it for step 3. The device can't read anything yet.

2. **Let it sync**, so the request reaches a trusted device.

3. **On a trusted device — approve it**, after
   [comparing fingerprints](#comparing-fingerprints). At launch the TUI shows an
   approval modal for any synced-in request; or use the command line:
   ```bash
   notema encryption device list                  # pending requests + fingerprints
   notema encryption device approve <name>         # or: --all, or: reject <name>
   ```
   Approving adds the device to the signed roster and **re-encrypts every entry**
   for the new recipient.

4. **Let it sync back.** Once the roster and re-encrypted entries arrive, the new
   device can read (unlock with its passphrase if it set one).

### Comparing fingerprints

The fingerprint is a short summary of a device's public key. A request is just a
file and grants nothing until a human approves it — comparing fingerprints before
you approve is what stops someone with write access to your synced folder from
sneaking a rogue device onto the roster.

The new device prints its fingerprint at `enroll` (and in `device list`); the same
fingerprint appears in the approving device's TUI modal and `device list`. Approve
**only** when the two match. Read the new device's fingerprint straight off its
screen — in person, over a call, or a message you trust — not from the synced
folder, since that's exactly what an attacker could have tampered with. If they
differ, reject the request.

### How a rolled-back roster is caught

The signed roster lives in the synced folder, so whoever hosts the sync could try
to serve an old copy — hiding a revocation, say, to keep a removed device trusted.
Each device guards against this with two local pins kept in `devices-trust.toml`
(in the config directory, never synced): the roster's genesis fingerprint and the
hash of the newest roster it has seen.

The pins are set the first time a device reads a valid roster (trust on first use)
and advance as the roster legitimately grows. On every later read the synced
roster is checked against them, and anything that doesn't extend the pinned
history — a swapped genesis, a truncated or rewound log — is rejected outright
rather than trusted. A brand-new device has no pin yet, which is why its first join
is anchored by the out-of-band fingerprint check above.

## Manage devices

```bash
notema encryption status                     # is it on, where's my key, who can read

notema encryption device list                # trusted devices + pending requests
notema encryption device rename OLD NEW      # relabel a device (no re-encryption)
notema encryption device revoke <name>       # revoke a device and re-encrypt without it

notema encryption key rotate                 # replace this device's key, retire the old one
notema encryption key passphrase             # add / change this device's key passphrase
notema encryption key passphrase --remove    # store the key unprotected
notema encryption key export <path>          # write a standalone copy, for safekeeping
```

Revocation is **forward-only**: re-encryption excludes the revoked device from
future entries, but any entries it already synced remain readable to it. Rotate a
device's key (or revoke and re-enroll) if you suspect its key was exposed.

Re-encryption also rewrites old entries. If those rewritten files sync over to the
revoked device and it kept no copy of the earlier ciphertext, it loses access to
those too — but don't count on that.

If encryption is disabled on one device (`notema encryption disable`), the other
devices notice on next launch, retire their local key material, and fall back to
reading the now-plaintext journal.

## Where your key lives

Two independent choices. **Format** — cleartext, or wrapped with a passphrase
(`key passphrase`). **Location** — where the bytes sit. Every location holds
either format, and moving the key never changes its format.

```bash
notema encryption key source file            # inline in identity.toml, mode 0600
notema encryption key source keyring         # the OS keychain
notema encryption key source command --read '<cmd>' [--store '<cmd>']
```

`encryption enable` and `device enroll` ask where to put a new key, and suggest
the keychain — macOS Keychain, Windows Credential Manager, or the Secret Service
on Linux and the BSDs — because it keeps the key out of a file that backups and
sync tools will happily copy. They fall through to the identity file without
asking where there is no keychain to reach: Android, the iSH build, a headless
Linux session with no D-Bus.

Without a terminal to ask on — a script, a container, a provisioning run — the
key stays in the identity file. That is the portable answer: a keychain chosen on
your behalf may be unreachable in the session that has to open the key. Pass
`--key-source keyring` to ask for it deliberately.

If the keychain turns out to be unreachable after all, `enable` says so and
leaves the key in the identity file rather than failing: by that point the
journal is encrypted and working, and the key is safe where it is.

Existing devices are left alone; move one over with `key source keyring` when you
want to.

> [!NOTE]
> On macOS the keychain grants access to the exact binary that created the item,
> so a release build is asked about once and not again. A binary you built
> yourself is unsigned and has no stable identity, so macOS re-asks after every
> rebuild.

`--read` prints the key on stdout whenever it's needed. `--store` takes it on
stdin whenever it changes — piping rather than passing an argument keeps the key
out of `ps`. Both are recorded, and both are needed for a complete setup:
without `--store` the key can be fetched but never replaced, so `key rotate`
and `key passphrase` have nowhere to write.

Before it drops the old copy, notema writes the key to its new home, reads it
back, and checks the same key comes out. A store command that quietly did nothing
can't leave you locked out. If that check fails, the copy it just wrote to the
keychain is removed again, so a failed move leaves nothing behind.

> [!WARNING]
> A `--read`/`--store` command is a shell line recorded in `identity.toml` and run
> every time the key is needed. Anyone who can write that file can run code as
> you. It is mode 0600 in your config directory, which is the protection — but if
> you sync your dotfiles, note that `identity.toml` stops being inert data the
> moment a fetch command is recorded in it, and treat a restored or copied-in
> identity file as you would any other executable content.

Moving a key *out* of a fetch command doesn't delete it from the secret manager —
that store is yours, not notema's, so it says what to clean up rather than
reaching into it. A key moved out of the OS keychain *is* removed, since notema
put it there.

Retrieval happens before the app takes over the terminal, so a command that
prompts — Touch ID, a GPG pin, a hardware token — works normally. It happens
once: a passphrase you then mistype is retried against the key already in hand,
never by fetching again from inside the TUI. `notema log "…"` never retrieves the
key at all: writing an entry needs only the public roster.

### Recipes

The command is a shell line, so anything that prints the bundle works. For the
macOS Keychain and the Secret Service, use `key source keyring` instead — it
talks to them directly.

| store | `--read` | `--store` |
| --- | --- | --- |
| 1Password | `op read op://Private/notema/identity` | `op document create --title notema/identity` |
| pass | `pass show notema/identity` | `pass insert -m notema/identity` |
| HashiCorp Vault | `vault kv get -field=identity secret/notema` | `vault kv put secret/notema identity=-` |
| a mounted secret | `cat /run/secrets/notema-identity` | `cat > /run/secrets/notema-identity` |

`key rotate` and `key passphrase` mint new key material and write it back
to wherever the key is kept, so they work the same in every location — provided
there's a way to write. A `--read` without a `--store` is the one case that
can't, and it refuses up front rather than part-way through a rotation.

> [!NOTE]
> While a rotation is in flight, notema keeps a self-contained rescue copy of the
> old key in your config directory — with the key inlined, since a pointer at a
> keychain item the rotation is about to overwrite would rescue nothing. It is
> deleted when the rotation finishes either way. A machine that dies mid-rotation
> leaves that copy on disk; it is mode 0600, and unprotected if your key is.

Creating a keychain-backed key also puts it in `identity.toml` first and moves it
after, so the move can verify the keychain hands it back before the local copy
goes away. The file is replaced, not shredded, so on most filesystems the
original blocks survive until they're reused.

## Disable encryption

```bash
notema encryption disable            # decrypts every entry and turns encryption off
```

Destructive encryption operations prompt for confirmation; pass `-y`/`--yes` to
skip the prompt in scripts.

## Recovery without the app

Encrypted entries decrypt with the standard [`age`](https://age-encryption.org)
tool and the age secret key from `identity.toml`, so you're never locked into
Notema to read your journal.

**1. Get the age secret key** (`AGE-SECRET-KEY-1…`).

If the key isn't inline — `identity.toml` has `keyring_account` or `keys_command`
instead of `plain_keys`/`encrypted_keys` — get it out first, either with
`notema encryption key export backup.toml` (which inlines it) or by
running the `keys_command` yourself. Then, from the inline form:

- *No passphrase* — copy the `x25519` value from the `plain_keys` block into a
  keyfile:
  ```bash
  printf 'AGE-SECRET-KEY-1...\n' > key.txt
  ```
- *Passphrase* — copy the armored `encrypted_keys` block
  (`-----BEGIN AGE ENCRYPTED FILE-----` … fences included) into `bundle.age`,
  decrypt it, then copy the revealed `AGE-SECRET-KEY-1…` line into `key.txt`:
  ```bash
  age --decrypt bundle.age    # prompts for the passphrase
  ```

**2. Decrypt an entry:**

```bash
age --decrypt --identity key.txt \
  "personal/2026/07/05/2026-07-05T14-30-00-<id>.md.age"
```

The output is the plaintext `.md` — front matter and body. The same key decrypts
every entry encrypted to this device. The `identity.toml` layout is in
[`docs/STORAGE-FORMAT.md`](STORAGE-FORMAT.md#encryption-files).
