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

## Micron 2400 after reboot (PR #7 driver)

The operator observed the following on the same disk. After a live driver reload the direct VPD 0x80 and PhysicalDisk serials both matched the profile. After reboot the ESP profile, direct device descriptor, embedded DUID descriptor, direct VPD 0x80 and Get-Disk all reported `D1F9V96CQUN4DJ7`. Get-PhysicalDisk returned `0000_0000_0000_0001_00A0_7523_453E_0948.`. Removing the separators gives the hex digits of the stable namespace ID `eui.000000000000000100A07523453E0948`. That correspondence makes a namespace-derived display value plausible. It does not identify the component that produced it.

### What the source establishes

- `install.ps1` registers SmbiosDiskFilter as a standalone kernel service with SYSTEM_START and error control IGNORE. Its configuration supplies no ordering guarantee relative to the first provider enumeration.
- In `driver/src/lib.rs`, DriverEntry accepts the first valid profile from the firmware variable or ESP fallback, runs registry synchronization, then calls `attach_all_storage_targets` once. It tries eight numbered disk paths, four RaidPort paths and Spaceport. It discards each attachment result and returns success after the sweep. There is no later device-arrival retry. A running service cannot identify which targets existed or attached at boot. The Spaceport comment describes an intended target, not an observed provider route.
- On an attached stack, only classified device-control requests with output capacity receive the patch completion routine. Successful completions use the smaller of returned bytes and requested capacity. Buffered SCSI pass-through interprets only SystemBuffer. The successful direct query demonstrates the observed result for that handle and request, without identifying which attachment handled it.
- Get-PhysicalDisk enumerates objects exposed by available Storage Management Providers. The earlier CIM trace identifies the class queried. It supplies neither the origin of the serial property nor the time it was populated. [Microsoft Get-PhysicalDisk documentation](https://learn.microsoft.com/en-us/powershell/module/storage/get-physicaldisk)

### Minimum read-only comparison

On the affected boot, run this once in an elevated PowerShell with the already-built diagnostic. Read the physical-disk provider first to keep the direct probe outside its observation window. Match by UniqueId, model and size. Stop if the match is ambiguous.

```powershell
Get-Date -Format o
Get-PhysicalDisk | Select-Object DeviceId,FriendlyName,Size,UniqueId,SerialNumber
Get-Date -Format o
Get-Disk -Number 0 | Select-Object Number,FriendlyName,Size,UniqueId,SerialNumber
.\storage-query.exe --vpd80
```

The expected split is recorded above. For the supplied results this comparison is already complete. Repeating it or querying service state cannot resolve the cause. For an **existing boot ETL**, open it in Windows Performance Analyzer and inspect the interval from device arrival through the first provider enumeration, then the timestamped provider-only window above if covered. Correlate activity/related IDs, device instance or target, process/thread and request/completion pairs where recorded. Separate the later direct probe. Apply these evidence criteria:

| Hypothesis | Observation that would distinguish it |
| --- | --- |
| Early enumeration and reuse | A correlated early acquisition of the grouped value followed by a provider cache hit or reuse of that object for the later call. Silence in an I/O trace alone is insufficient. |
| Attachment timing | Evidence of a failed or premature attachment to the specific target later used by the provider, or a target arriving after the one-time sweep. Driver image load alone does not timestamp the attachment. |
| Alternate request, target or value source | A correlated provider-side acquisition on another target, via another request, or a recorded derivation from a namespace ID. Matching text alone leaves all three open. |
| Other boot initialization | First population by another component before the provider call, followed by a traceable handoff. Compare boot and existing reload traces only when the same device and events are identifiable. |

A CIM-only trace or an ETL lacking the needed attachment, acquisition or cache events leaves the cause unresolved. The current filter does not emit per-target attachment outcomes. No new kernel instrumentation or boot trace configuration is required by this comparison.

### Separate MachineGuid observation

After profile regeneration with R, the operator observed the previous UUID in MachineGuid. DriverEntry prefers a valid firmware profile to the ESP copy and attempts a registry write for a nonzero loaded UUID. The helper does not report key-open or value-write failures to DriverEntry. The installer also has a separate synchronization path using the Windows-reported SMBIOS UUID. The current disk serial alone cannot establish which UUID was loaded, whether that registry write succeeded, or which writer ran last. Compare the recorded old/new profile UUIDs to these read-only values from the same boot:

```powershell
(Get-CimInstance Win32_ComputerSystemProduct).UUID
(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid
```

Both current values establish a snapshot only. Determining writer order requires corresponding historical evidence. This issue is independent of the storage-provider investigation.
