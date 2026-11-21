# Windows storage property diagnostic

By default this utility opens `\\.\PhysicalDrive0` with zero requested access and queries
`StorageDeviceProperty`, `StorageDeviceIdProperty`, and
`StorageDeviceUniqueIdProperty` using `IOCTL_STORAGE_QUERY_PROPERTY`.
The default run issues only those three metadata queries. It needs no repository driver
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

## Optional SCSI Unit Serial Number page

Run from an elevated Windows shell if you want to query EVPD page `0x80` as well.

```powershell
cargo run --locked --manifest-path tools/storage-query/Cargo.toml --target x86_64-pc-windows-msvc --target-dir target -- --vpd80
```

Windows requires read **and** write access to the disk handle for
`IOCTL_SCSI_PASS_THROUGH`, even for a data-in command. The flag opens a second
handle with those permissions, reads the adapter's alignment requirement, and
sends one fixed, synchronous, data-in SCSI INQUIRY for page `0x80` through the
physical disk's class-driver path. The command accepts no custom CDB, sends no
data-out or native NVMe passthrough commands, and performs no writes.
[Pass-through access and routing](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddscsi/ni-ntddscsi-ioctl_scsi_pass_through) · [Physical disk access](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew#physical-disks-and-volumes)

Microsoft documents INQUIRY-to-Identify translation for StorNVMe. The NVMe
SCSI Translation Reference, revision 1.5, describes an NGUID-based page `0x80`
serial as eight groups of four hex digits with a final dot, matching the
shape of the reported Micron value. Support still depends on the Windows stack
and driver servicing the device. A denied handle, unsupported request, or
non-good SCSI status only marks the optional probe unavailable and leaves the
base command's exit status unchanged.
[StorNVMe translation](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/stornvme-scsi-translation-support) · [NVMe SCSI Translation Reference, §6.1.3](https://nvmexpress.org/wp-content/uploads/NVM_Express_-_SCSI_Translation_Reference-1_5_20150624_Gold.pdf)

The probe prints the exact returned INQUIRY data as hex and the bounded page
payload as escaped serial text. A successful or matching page `0x80` reply
describes this direct request alone. To establish the internal request path or
look for caching in `MSFT_PhysicalDisk`, correlate a separate `Get-PhysicalDisk`
call with WMI activity and available storage I/O traces. This is also the
read-only alternative when pass-through is unavailable. See the trace plan in
the [handoff](../../handoff.md).

### Interpreting a page `0x80` result with the filter

On the tested Micron 2400, the direct page `0x80` reply matched the grouped
`MSFT_PhysicalDisk.SerialNumber` exactly. The direct storage-property serial,
DUID embedded serial, `Win32_DiskDrive`, and `MSFT_Disk` instead matched the
configured serial. The VPD `0x83` identifier remained valid. Matching strings
still leave the provider's device stack, IOCTL choice, and cache behavior open.

The x64 filter recognizes only buffered, data-in EVPD `0x80` INQUIRY replies
whose request, completion status, transfer length, and page bounds validate.
It can replace an ASCII unit-serial field such as the grouped Micron string.
VPD `0x83`, EUI-64 and NGUID identifier fields, `eui.` strings, and unrelated
SCSI requests pass through unchanged. The filter does not decode WOW64
pass-through headers on its x64 path.

Before comparing behavior on hardware, retain the unmodified baseline results
from the direct queries and provider objects. For any subsequent hardware
test, confirm the buffered completion crosses the intended filter and record
the NTSTATUS, `IoStatus.Information`, output capacity, data offset, and transfer
length. The filter uses only the smaller of the byte count and capacity. A
header-only or incomplete completion therefore keeps the original payload. A
changed direct page `0x80` serial can coexist with an unchanged provider value.
Compare page `0x83` and the namespace identifiers byte-for-byte against the
baseline as well.

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
