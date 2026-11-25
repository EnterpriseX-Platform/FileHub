# Encryption at rest

Every folder can choose how its files are encrypted when it is created
(New folder → Encryption), and the choice shows in the folder's details.

| Choice | How a file is stored | Key |
|---|---|---|
| Basket default | as the basket says (with `STORAGE_ENCRYPT_ALL`, symmetric) | — |
| Symmetric (AES-256) | AES-256-GCM, chunked (format `FHXENC02`) | `STORAGE_ENC_KEY` |
| Asymmetric (RSA) | a random AES-256 data key per file encrypts the file; the data key is encrypted with the server's RSA public key (RSA-OAEP, SHA-256) and stored with the file (format `FHXENC03`) | `STORAGE_RSA_PRIVATE_KEY_FILE` |

FileHub holds both keys, so people open, preview and search files as usual.

## Where the RSA key lives

The private key is never written to the database. Keep it in a secret store of
its own (for Kubernetes, a Secret separate from the application's settings,
mounted as a file) and point `STORAGE_RSA_PRIVATE_KEY_FILE` at it.
`STORAGE_RSA_PRIVATE_KEY` (the PEM itself) works too.

Create a key:

```
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out filehub-rsa.pem
```

## Backup and recovery

Without the private key, files in asymmetric folders can't be opened by anyone.

- Back up the key when it is created: two copies, offline and in separate places
  (for example a sealed USB key in a safe, and the organisation's key vault).
- Back up `STORAGE_ENC_KEY` the same way; it protects every symmetric file.
- To recover, put the saved PEM back where `STORAGE_RSA_PRIVATE_KEY_FILE`
  points and restart the API. The log says `RSA key loaded` when it is read.
- Check the backup now and then: restore it on a test server and open a file
  from an asymmetric folder.

Changing the RSA key makes earlier asymmetric files unreadable, so keep the old
key as long as those files exist.
