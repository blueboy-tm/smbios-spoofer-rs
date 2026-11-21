# Experimental Windows Storage UniqueId alias patch

This patch is **opt-in** and **specific to the observed Micron 2400**.
It does not rewrite NVMe firmware, namespace Identify payloads, on-disk GPT
IDs, genuine EUI-64 / NGUID, or the Spaceport provider's independent output.

## Observed baseline

The user's `\\.\PhysicalDrive0` direct `IOCTL_STORAGE_QUERY_PROPERTY`
replies showed:

- `StorageDeviceProperty`: already-spoofed serial `D1F9V96CQUN4DJ7`
- `StorageDeviceIdProperty` (PropertyId 2): one UTF-8, device-associated,
  36-byte SCSI name-string identifier:
  `eui.000000000000000100A07523453E0948`
- `StorageDeviceUniqueIdProperty` (PropertyId 3): spoofed embedded serial
  and **the same** 36-byte identifier, with no layout signature.

The new `namespace-id-patch` driver Cargo feature performs an exact-match,
bounded replacement of **only those identifier bytes**. It validates the
descriptor's size, one-identifier count, CodeSet=3, IdentifierType=8,
Association=0, identifier length=36, NextOffset=0, and exact old value.
In the DUID it also validates the section offsets and keeps the existing
embedded-serial rewrite untouched.

The replacement is `eui.` followed by 32 uppercase hex digits derived
deterministically from the persisted v9 serial, so the two IOCTL properties
produce the **same synthetic string** after reboot. **This is a private
synthetic alias, NOT a real IEEE-assigned EUI or NVMe NGUID**, and its
non-cryptographic derivation does not provide globally guaranteed uniqueness.
If the response differs from this one machine's exact ID and structure, the
namespace patch does nothing. The legacy serial-only path stays unchanged
without the feature.

## Test the optional combined patch

On an authorized Windows test machine, save the pre-change output, review
the test module, and build using the existing installer:

```powershell
git switch main
git pull --ff-only origin main
.\install.ps1 -Mode build-only -DriverFeature "spaceport-serial-patch,namespace-id-patch"
.\install.ps1 -Mode install-driver -DriverFeature "spaceport-serial-patch,namespace-id-patch"
```

The installer signs a new immutable driver image, updates the kernel service
to load it at **the next boot**, and does **not** live-stop/reload the filter.
Restart Windows, then run:

```powershell
.\install.ps1 -Mode audit-drivers
cargo run --release --manifest-path tools/storage-query/Cargo.toml --target x86_64-pc-windows-msvc
Get-Disk | Select-Object Number,SerialNumber,UniqueId
Get-PhysicalDisk | Select-Object FriendlyName,SerialNumber,UniqueId
```

**Expected only on the two direct properties**: the descriptor's old
`eui.` string is replaced with the same deterministic synthetic value, and
the serial is still `D1F9V96CQUN4DJ7`.

**Not yet established**: that Get-Disk, Get-PhysicalDisk, storage WMI, NVMe
Identify and VPD 0x83 all use this same ID. They may report the original,
causing cross-layer inconsistencies. Check disk stability and all outputs
rather than asserting success from a single query.

The existing `spaceport-serial-patch` feature does not imply
`namespace-id-patch`; the new feature must be explicitly requested.
To revert to the previously tested serial-only behavior:

```powershell
.\install.ps1 -Mode install-driver -DriverFeature spaceport-serial-patch
```

Then restart normally. Do not manually stop the loaded WDM filter.
