# Retail Kontakt library decryption

Cadence can decode encrypted retail `.nki`/`.nkx` content when a licensed
Kontakt 8 installation is present. This covers the preset subtree and the sample
monolith. It does not install or modify Kontakt, and it writes nothing to the
library.

## Scheme (recovered from the saved Kontakt 8 REA export)

| Piece | Saved function | Behavior |
| --- | --- | --- |
| Product key lookup | `NI::SOUND::EncryptionTraits::getEncryptionKey` `0x102516a18` | Reads product-specific `HU` and `JDX` hex values for the instrument's SNPID |
| Pad derivation | `BLibraryInfo::InitOTP` `0x1002b7554`, `aesCtr` `0x10022a240` | `pad = AES-CTR(key = JDX, iv = HU)` over the static 64 KiB `bEncOneTimePad` seed |
| Preset subtree | `EncryptionItem::decompressData` `0x10252b24c` | XOR the ciphertext with the pad, then decompress with FastLZ |
| Samples | `BSample::InitDecryption` `0x10043d668`, `BFileEncrypter::DecryptBuffer` `0x10022f858` | Monolith entries with encryption mode `0x100` XOR sample bytes with the library pad starting at offset 0 |

The pad is 64 KiB and repeats. `JDX` is 16, 24, or 32 bytes (AES-128/192/256);
these libraries use 32-byte keys (AES-256-CTR). `HU` is the 16-byte IV.

## Key discovery

The `HU`/`JDX` values ship in the library's `.nicnt` metadata inside
`<ProductSpecific>`, alongside `<SNPID>` and `<RegKey>`. Kontakt normally reads
the same values from the NI machine-key registry (`com.native-instruments.<RegKey>`,
`openLibraryMachineKey` `0x1002b7740` / `GetHEX` `0x1002b7848`), with a hardcoded
`encryptionInfo` table as fallback. Cadence reads the `.nicnt` directly and
matches the instrument's authorization PID.

The static 64 KiB seed is read at runtime from the installed Kontakt binary
(`/Applications/Native Instruments/Kontakt 8/Kontakt 8.app/Contents/MacOS/Kontakt 8`).
That binary is universal; Cadence selects the arm64 slice and resolves
`g_aPrivateKey`/`bEncOneTimePad` through the Mach-O symbol table. The seed is
never copied into Cadence. `CADENCE_ENC_PAD` can point at a raw seed file for
testing.

## Implementation

- `crates/nk-format/src/encryption.rs` — `.nicnt` parsing, AES-CTR (macOS
  CommonCrypto), universal Mach-O seed extraction, pad derivation/caching.
- `crates/nk-format/src/ni_container.rs` — `parse_with_pad`: try plain FastLZ,
  then XOR with the pad and retry FastLZ.
- `crates/nk-format/src/loader.rs` — key discovery after an encrypted preset
  fails, `.nkx` monolith reference resolution, and resident sample decryption.
- `crates/nk-format/src/stream.rs` — `SampleReader::open_with_pad` XORs page
  reads, so streaming decrypts the same sample bytes as the resident path.
- `crates/sample-adapter/src/lib.rs` — passes the library pad into `stream_all`.

## Validation

Environment: external T7 SSD, Kontakt 8.14.1 installed (universal binary),
Logic 12.3.1.

| Library | Result |
| --- | --- |
| **GroveBass** (SNPID `Y55`) | Preset decodes to 1 program, 10 groups, **5,742 zones**; resident render peak 0.499 (float), 16,327 (i16 WAV) |
| **Jacob Collier Audience Choir** (`02K`) | Streamed and resident renders are **byte-identical** (SHA-256 `f587d94c…`); 158 samples preloaded, 76.0 MiB resident |
| **Krimh Drums Free** (`U90`) | Note 36 renders 3 voices; note 60 is not mapped (drum kit) |
| AES-CTR vector | GroveBass pad first 32 bytes match the OpenSSL AES-256-CTR result |

`encrypted_libraries_load_with_keys_or_fail_explicitly` in
`crates/sample-adapter/tests/library_stress.rs` loads all three retail libraries
when the seed and keys are available, and still reports the explicit
encryption/key error otherwise.

## Limits

- Retail decryption currently uses macOS CommonCrypto for AES-CTR. Other
  platforms return an explicit "requires macOS CommonCrypto" error until a
  portable AES is added.
- The static seed must come from a licensed Kontakt installation (or
  `CADENCE_ENC_PAD`). Without it, encrypted libraries fail with a clear error.
- GroveBass has 5,742 zones; `stream_all` still hits the 128 MiB resident
  preload budget because it preloads every zone attack. Resident loading works.
  A demand-based/shared preload policy is still needed for huge streaming
  libraries.
- `.nkc` collection containers remain undecoded; the `.nkx` monolith and `.ncw`
  paths cover the validated libraries. Encrypted sample modes other than
  `0x100` (built-in `BMonolith::xorKeys`) are not needed by these libraries and
  remain unimplemented.
- Kontakt scripting/effects are still separate from file decoding. Native
  decoding gives Cadence the same zones/samples, not Kontakt's script engine.
- NI's EULA restricts reverse engineering; this is interoperability research on
  libraries the user owns.
