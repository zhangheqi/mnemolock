# mnemolock

CLI utility for encrypting BIP39 mnemonics with a password.

The encrypted mnemonic is made of BIP39 words too, so you can write it down and store it like the
original: a 12-word mnemonic encrypts to 24 words, and a 24-word mnemonic to 36 words.

## Usage

```sh
mnemolock encrypt   # enter a 12- or 24-word mnemonic, then choose a password
mnemolock decrypt   # enter the 24 or 36 encrypted words, then the password
```

Words and passwords are masked as you type. Use the arrow keys to move between words, Space to go
to the next word, Enter to submit, and Esc or Ctrl+C to quit.

The encrypted words are only as safe as your password. The salt is the same for everyone, so an
attacker can test a guessed password against every mnemolock backup at once. Use a long password
that you don't use anywhere else.

## Format

This section describes exactly how mnemolock encrypts a mnemonic, so you can recover it with other
tools if mnemolock is ever unavailable. None of these values can change without making existing
encrypted mnemonics undecryptable.

1. **Entropy.** Decode the English BIP39 mnemonic to its entropy: 16 bytes for 12 words, 32 bytes
   for 24 words.
2. **Key.** Derive a 32-byte key from the password with Argon2id, version 0x13:
   - Password: the UTF-8 bytes of the password, exactly as typed, with no Unicode normalization.
   - Salt: the 22 ASCII bytes `Uom1dTQVPOMypdOKji8axA`, used as they are (not base64-decoded).
   - Memory: 19456 KiB (19 MiB).
   - Iterations: 2.
   - Parallelism: 1.
   - No secret and no associated data.
3. **Encrypt.** Encrypt the entropy with AES-256-GCM-SIV ([RFC 8452]), using that key, the 12 ASCII
   bytes `j+d5//euNzoM` as the nonce, and no associated data. The output is the ciphertext followed
   by the 16-byte tag: 32 bytes for a 12-word mnemonic, 48 bytes for a 24-word one.
4. **Encode.** Encode the output as English BIP39 words, using it as entropy:
   - 32 bytes: one 24-word mnemonic.
   - 48 bytes: bytes 0–31 as a 24-word mnemonic, followed by bytes 32–47 as a 12-word mnemonic,
     for 36 words in total.

To decrypt, reverse the steps. The word count tells you the original length: 24 encrypted words
hold a 12-word mnemonic, and 36 hold a 24-word one. Decryption fails if the password is wrong.

Each part of the encrypted mnemonic is a valid BIP39 mnemonic with its own checksum, so most typos
are caught before decryption.

[RFC 8452]: https://www.rfc-editor.org/rfc/rfc8452

### Test vectors

12-word mnemonic:

- Mnemonic: `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
- Password: `correct horse battery staple`
- Encrypted: `feature chapter top slight shrimp ranch combine tomato bike canvas volume vendor upper rich very rhythm aspect leaf fame trouble salad bench slow peasant`

24-word mnemonic:

- Mnemonic: `legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title`
- Password: `pässwörd 🔒` (UTF-8 hex `70c3a4737377c3b6726420f09f9492`)
- Encrypted: `inquiry because trip cabin lottery fire syrup auction sample crazy eye cave cabbage mixture wrist embody talk airport cart reflect fluid ecology amused embark nose test axis scene enlist useless seven alpha gift bulk ivory tribe`
