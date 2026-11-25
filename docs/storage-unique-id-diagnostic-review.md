# Windows Storage UniqueId diagnostic review

## Executive Summary

The operator observed a changed disk serial and the original UID across all
six reported paths. The observations establish a baseline for further work.
They leave feature activation and the source of the provider's UID unresolved.

## Repository State

[PR #18](https://github.com/blueboy-tm/smbios-spoofer-rs/pull/18) was open on
`feat/guarded-storage-unique-id-patch` at `0dbddb8` against `main` at
`71a8d8a`. [PR #16](https://github.com/blueboy-tm/smbios-spoofer-rs/pull/16)
was merged in `3836322` and concerns the Spaceport serial path. The GitHub
connector returned no submitted reviews, issue comments or PR-triggered
workflow runs for the reviewed #18 head. This leaves CI status unverified.

The operator built the user-mode tool from `277fa62017dbee9196ed6a385c20942c74f8fd52`.
The later `0dbddb8` restricts the optional UID completion path to a valid
SystemBuffer, rejects lengths exceeding capacity, tightens counter accounting
and corrects the checkout guide. Local validation below used `0dbddb8` plus
this diagnostic test. Neither source SHA identifies the loaded Windows image.

## Verified Test Environment

Operator-reported Windows kernel build 26100, x64, build lab
`26100.1.amd64fre.ge_release.240331-1435`, 16 logical processors.
The independently matched Micron 2400 (`Micron_2400_MTFDKBA512QFM`) is Disk 0,
PhysicalDisk DeviceId 0 and `\\.\PhysicalDrive0`. Both views correspond to NVMe,
512110190592 bytes and `PCI Slot 2 : Bus 3 : Device 0 : Function 0 : Adapter 0`.
The match relies on bus, size and location, independent of UID and serial.

The on-disk `C:\Windows\System32\mispace.dll` has file and product version
`10.0.26100.8457` and SHA-256
`DD0436542EDE68A75EF9CD2BC8E06A9B2116EB2B94942F7A2B83F7D85AA00DFE`.
This identifies the file examined on disk.

## Evidence E01–E10

| Evidence | Operator observation |
| --- | --- |
| E01 | Both cmdlets reported serial `0000_0000_0000_0001_00A0_7523_453E_0948.` and the original UID in SCSI Name String format. |
| E02 | Both cmdlets reported serial `ZNGR2179MCBYD1M` afterward. UID and format were unchanged. |
| E03 | SmbiosDiskFilter was Running with System start. Configured image `C:\WINDOWS\System32\drivers\smbios-disk-filter-r336ff19332f58554.sys` had a valid signature. Profile version was v9. |
| E04 | `cargo build --locked --release --manifest-path tools/storage-query/Cargo.toml --target x86_64-pc-windows-msvc` succeeded on Windows for the user-mode tool at `277fa62`, then the tool queried PhysicalDrive0. |
| E05 | StorageDeviceProperty returned `ZNGR2179MCBYD1M` with trailing spaces via SerialNumberOffset. |
| E06 | StorageDeviceIdProperty returned the original 36-byte UTF-8 SCSI name string, CodeSet 3, IdentifierType 8, device association 0. |
| E07 | DUID returned the changed embedded serial and the same identifier bytes as E06. No embedded layout signature was reported. |
| E08 | Local WinDbg prompt `lkd>`. `vertarget` showed build 26100. `lm` showed only `nt`. `lmv m *smbios*` and `x *!SmbiosNamespaceIdCounters` found no matches. |
| E09 | CIM metadata for MSFT_PhysicalDisk, MSFT_StorageObject and MSFT_Disk described UniqueId as a required read-only string allowing null and UniqueIdFormat as UInt16. ModelCorrespondence referenced MSFT_StorageObject.UniqueId. ValueMap was 0, 1, 2, 3, 8. |
| E10 | Both direct CIM classes reported the original UID and format 8 for the same physical location. |

E06/E07 raw hex:
`6575692E3030303030303030303030303030303130304130373532333435334530393438`.

## Storage Identity Comparison

Original UID is `eui.000000000000000100A07523453E0948`.

| Path | Baseline | Observed after | Status |
| --- | --- | --- | --- |
| StorageDeviceIdProperty | Not collected separately in E01 | Original UID | Original value observed |
| DUID identifier | Not collected separately in E01 | Original UID | Original value observed |
| Get-Disk.UniqueId | Original UID | Original UID | Unchanged |
| Get-PhysicalDisk.UniqueId | Original UID | Original UID | Unchanged |
| MSFT_Disk.UniqueId | E01 cmdlet baseline | Original UID, format 8 | Agrees with cmdlet |
| MSFT_PhysicalDisk.UniqueId | E01 cmdlet baseline | Original UID, format 8 | Agrees with cmdlet |

The `eui.` prefix is part of the reported UTF-8 text. Descriptor IdentifierType 8
and CIM UniqueIdFormat 8 describe SCSI Name String representation. Neither
establishes that the 36 bytes are a binary EUI or identifies where the provider
obtained the string. Equal outputs also fit independent sources.

## Parser Safety Review

In `src/namespace_id_patch.rs`, the direct parser bounds the declared length
and checks the single identifier's type, encoding, association, chain and exact
old text before writing. DUID validation checks header version, size and offsets.
The test exercises every truncated descriptor prefix, oversized declared length,
ambiguous count, unexpected type/encoding/association and nonmatching old
identifier. Rejected buffers are compared byte for byte. The value guard is
not independent disk identification. The direct descriptor parser does not check
the leading Version field, and the DUID parser uses adjacent section start
offsets without validating that an earlier section's declared extent ends before
the ID section. These are limits of the existing checks, not runtime findings.

`driver/src/namespace_id_diagnostics.rs::valid_buffered_output` tests successful
status, nonzero completed length, capacity and buffer presence. Source review
of `driver/src/lib.rs` shows that UID-tagged requests skip the MDL branch on
`0dbddb8`. Existing synthetic DUID tests protect bytes adjacent to its ID
region. They do not provide an independent live AdapterSerialNumber check.
The UID parser is scoped to the two storage properties. Identify, VPD 0x83,
binary namespace identifiers and GPT are outside that mutation path. This is
a source-scope observation and does not substitute for hardware regression tests.

## Test and Build Results

Linux x86-64, rustc 1.100.0 (0ef1bc386 2026-11-09), reviewed head `0dbddb8`
with the diagnostic test applied. In the commands below, MODE was executed
once as `--no-default-features` and once as `--all-features`. The Windows
driver was cross-linked with rust-lld and the repository's import definition.

| Command | Exit and result |
| --- | --- |
| `cargo test --locked --target x86_64-unknown-linux-gnu MODE -q` | 0, 158 tests each |
| `cargo clippy --locked --manifest-path driver/Cargo.toml --target x86_64-pc-windows-msvc MODE -- -D warnings` | 0, both configurations |
| `cargo build --locked --release --manifest-path driver/Cargo.toml --target x86_64-pc-windows-msvc MODE` | 0, both configurations |
| `cargo test --locked --manifest-path tools/storage-query/Cargo.toml --target x86_64-unknown-linux-gnu -q` | 0, 15 tests |
| `rustc --edition=2024 --test src/namespace_id_patch.rs -o ../uid-review-tests` and `../uid-review-tests` | 0 and 0, 5 tests |
| `rustc --edition=2024 --test driver/src/namespace_id_diagnostics.rs -o ../uid-review-tests` and `../uid-review-tests` | 0 and 0, 3 tests |
| `cargo fmt --all --check` | 1, existing formatting differences across the tree |
| `git diff --check` | 0 |

The standalone runs overlap with available module coverage. Driver linking
and synthetic fixtures do not validate runtime kernel behavior.

## Changed Files

- `docs/storage-unique-id-diagnostic-review.md` records the evidence and validation.
- `src/namespace_id_patch.rs` adds a test module compiled only under `cfg(test)`.

## Unverified Claims

| Claim | Status and missing evidence |
| --- | --- |
| Loaded module revision | NOT VERIFIED. Source checkout and configured signed path do not identify the loaded PE. |
| Loaded feature flags | NOT VERIFIED. Service state and serial behavior cannot establish the UID build flag. |
| Counter access | NOT VERIFIED. The debugger's limited module list and missing symbol give no usable counter reading. |
| MISpace UID source | NOT VERIFIED. On-disk binary identity and CIM metadata do not show property population. |
| Shared IOCTL/provider origin | NOT VERIFIED. Equal strings provide no provenance. |
| IOCTL 0x00E70408 association with UID | NOT VERIFIED. Existing Spaceport serial evidence does not trace UID consumption. |
| Offset 0xA3C association with UID | NOT VERIFIED. Prior observations relate to a serial candidate. |
| Replacement persistence | NOT VERIFIED. No replacement UID was observed. |
| Hardware acceptance | NOT VERIFIED. PhysicalDisk reported the original string. |

## Limitations and Blockers

The smallest evidence gap is attribution of the loaded image and feature set,
then a supported diagnostic reading for each direct query. Independent proof
of the value consumed by MISpace is required to attribute the provider path.
No additional Windows operations were performed for this review.

## Final Status

**DIAGNOSTIC BASELINE VERIFIED — UNIQUEID CHANGE NOT OBSERVED**

**PR objective: BLOCKED — INSUFFICIENT EVIDENCE**
