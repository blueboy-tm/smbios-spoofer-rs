# Windows storage property diagnostic

This utility opens `\\.\PhysicalDrive0` with zero requested access and queries
`StorageDeviceProperty`, `StorageDeviceIdProperty`, and
`StorageDeviceUniqueIdProperty` using `IOCTL_STORAGE_QUERY_PROPERTY`.
It issues only those three metadata queries. It needs no repository driver
installation, reload, or service changes.

From the repository root on x64 Windows:

```powershell
cargo run --locked --manifest-path tools/storage-query/Cargo.toml --target x86_64-pc-windows-msvc --target-dir target
```

Confirm that `PhysicalDrive0` is the device you intend to inspect. If access
is denied, run the diagnostic from an elevated shell. The output identifies
each query separately. Unsupported queries, malformed descriptors, and Win32
errors are reported per query. Replies are capped at 1 MiB. The command exits
with an error if the handle cannot be opened or none of the replies can be decoded.

The tool prints the raw serial as escaped text and as hex bytes, excluding the
NUL terminator. It prints VPD identifier code sets, types, associations, raw
bytes, and any decodable text. Binary designators retain their original bytes
and length. The DUID query requests hardware IDs. The utility labels embedded
device serials and VPD IDs separately, along with a layout signature if one is
returned. The DUID is a structured response with no single provider-style
`UniqueId` string.

## Compare the same disk across APIs

```powershell
Get-CimInstance Win32_DiskDrive |
    Select-Object DeviceID, PNPDeviceID, Model, Size, SerialNumber

Get-Disk |
    Select-Object Number, Path, Location, FriendlyName, Size, SerialNumber, UniqueId, UniqueIdFormat

Get-PhysicalDisk |
    Select-Object DeviceId, ObjectId, PhysicalLocation, FriendlyName, Model, Size, SerialNumber, UniqueId, UniqueIdFormat
```

1. Match the `Win32_DiskDrive.DeviceID` of `\\.\PhysicalDrive0` to the tool.
   Correlate the provider objects by path or physical location, model, and
   size. A provider's `DeviceId` may use a different numbering scheme.
2. Compare each serial to the direct `StorageDeviceProperty` serial. Use the
   two ID queries to compare the reported text or bytes with the provider's
   `UniqueId` and `UniqueIdFormat`. Preserve differences in punctuation and
   padding when recording results.
3. Record the provider results and direct queries separately. The Micron
   observation had the grouped serial in `MSFT_PhysicalDisk`, the profile
   serial in `MSFT_Disk`, and the same `eui.` ID in both. The direct queries
   describe the selected disk handle. Even matching values do not establish
   either provider's internal request path or cache behavior. Use separately
   correlated provider and storage I/O traces for that question.

[Storage property query and descriptor contract](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-ioctl_storage_query_property) · [DUID layout](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/storduid/ns-storduid-_storage_device_unique_identifier)
