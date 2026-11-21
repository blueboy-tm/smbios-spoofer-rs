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
.\target\x86_64-pc-windows-msvc\debug\storage-query.exe --vpd80
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

### Opt-in boot attachment snapshot

The driver feature `attachment-diagnostics` retains 13 one-time results in a
208-byte fixed array in the loaded driver image. DriverEntry is the only writer.
Each row records the interrupt time just after the helper returned, then
publishes the outcome with a release store. There are no diagnostic callbacks,
request counters, serials, pointers, file writes or writes from completion.
The default driver has no snapshot export or diagnostic clock call.

For a separately authorized hardware verification, the affected boot must
already use the feature build. Use an existing kernel debugger session or a
kernel dump from that boot that contains the loaded driver's data pages.
A user-mode debugger, a normal build, or a dump captured after unload cannot
supply the snapshot. No debugger setup or boot setting changes are part of
this procedure. In WinDbg, run:

```text
x *!SmbiosAttachmentSnapshot
dq <address returned by x> L1a
```

Read the 26 hexadecimal quadwords as 13 consecutive pairs. In order, rows
0–7 represent the named HarddiskN/DRN attempts, 8–11 represent RaidPort0–3,
and 12 represents Spaceport. The first word is interrupt time in 100 ns units
since boot (divide by 10,000,000 for seconds). The high 32 bits of the second
word encode the stage below. The low 32 bits preserve the call's NTSTATUS;
WinDbg `!error <hex status>` can decode it. A zero status at a failed stage
can indicate a missing output pointer.

| Stage | What the helper observed |
| --- | --- |
| 0 | No published result for this load at capture time. |
| 1 | Attachment table full before lookup; no DDI status. |
| 2 | Named-device lookup failed or returned a null target. |
| 3 | Filter-device creation failed or returned a null device. |
| 4 | Stack attachment failed or produced no lower device. |
| 5 | Stack attachment succeeded and the filter was recorded in the attachment table. |

On the already-confirmed split, collect the snapshot once. A name-not-found
result followed by a device-arrival record for that same target supports late
availability. Compare timestamps only after aligning the trace to the same
boot clock. A successful row rules out a missed attachment *for that named
attempt*. It establishes neither the provider's request target nor when the
provider acquired its serial. Even all-success rows leave earlier enumeration,
reuse and a different acquisition path open. Use the provider evidence criteria
above before selecting a correction. Repeating the reload would erase this
load's snapshot and change the condition under investigation.

The existing unload path marks unloading, detaches recorded devices, spins
for a bounded interval for counted completions and then deletes filter devices.
That timeout does not guarantee all I/O has drained. The snapshot adds no
outstanding work or references. Any future arrival subscription would need
independent callback rundown, removal handling and duplicate-attachment
protection. The reported values alone do not justify that lifecycle change.

Offline snapshot-format regression test from the repository root:

```sh
rustc --edition 2024 --test driver/src/attachment.rs -o attachment-tests
./attachment-tests
```

[Windows interrupt time contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-kequeryinterrupttimeprecise)

### PowerShell access and driver lifecycle review

The debugger export remains the retrieval interface. The current driver only
owns attached filter devices. Its common dispatch paths expect a filter
extension and a lower device. A standalone control device would need its own
dispatch routing for create, cleanup, close, device control and unsupported
requests. It would also need a separate lifetime model. The existing completion
counter covers selected storage requests, and the unload loop can stop waiting
while that counter is still nonzero.

A future control device should use a Windows-supported secure creation path
such as IoCreateDeviceSecure, a dedicated class GUID, a protected DACL granting
access to SYSTEM and Built-in Administrators, and FILE_DEVICE_SECURE_OPEN.
That API uses the WDK Wdmsec library, outside the current import-only build.
Its effective security descriptor and open behavior should be verified on
Windows with SYSTEM, elevated administrators, filtered administrator tokens,
and standard users, including opens with trailing path components. A
PowerShell check can supplement those tests but cannot enforce the policy.

A proposed protocol would allow one read-access METHOD_BUFFERED request,
require zero input bytes, and return an explicit version and exactly 13
serialized records. All other requests should fail with zero returned bytes.
Initialization would need to publish the device only after it is ready and
roll back both the symbolic link and the device on failure. Removing the link
blocks opens through that link only. Existing handles and in-flight requests
need a lifetime guarantee, including a Windows test that exercises unload with
an open handle and concurrent reads. The storage completion counter cannot
provide that guarantee for a standalone device.

The available Linux cross-build environment cannot establish those runtime
access and unload properties, and this review authorizes no driver install or
restart. Use the WinDbg snapshot procedure above for the minimum hardware
comparison. On a separately authorized, reviewed diagnostic build, boot once,
capture the snapshot before unload and compare Get-PhysicalDisk with direct
VPD 0x80 on the same boot. A successful attachment still leaves the provider's
request path unresolved.

[Windows device security API](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdmsec/) · [Device characteristics](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/specifying-device-characteristics)

### Correlate a PhysicalDisk query with ETW

The reported diagnostic boot attached Harddisk0/DR0, RaidPort0 and Spaceport
(stage 5, status zero). Harddisk1–7 failed lookup with 0xC000003A and
RaidPort1–3 with 0xC0000034. This rules out a missed attachment to the three
successful named targets during the sweep. The provider's request path and
the time that it acquired the serial remain unknown.

Microsoft documents the PhysicalDisk serial as matching the disk's SCSI
inquiry data, and lists Inquiry-to-Identify translation in StorNVMe. The
observed grouped value `0000_0000_0000_0001_00A0_7523_453E_0948.` has the same
hex digits as `eui.000000000000000100A07523453E0948` when separators are
removed. Earlier hardware results also observed that grouped text directly
in VPD 0x80. A namespace-derived serial from translation is therefore
plausible. The text does not locate the transformation or establish that
Storage Spaces, StorNVMe or a particular cache furnished this instance of
the property.

From Windows PowerShell at the repository root, run the following on the
affected boot. Elevate if Windows denies ETW session access.

```powershell
.\tools\Trace-PhysicalDisk.ps1
```

The script supports Windows PowerShell 5.1. It starts a uniquely named 8 MB
circular ETL and waits up to eight seconds for one query. It stops the session
in cleanup and retries a failed stop. If both attempts fail or the PowerShell
process is forcibly terminated, use `logman query -ets` to find the session
named `PhysicalDisk-<id>`, then run `logman stop PhysicalDisk-<id> -ets` for
that session only. The ETL path is printed on cleanup. The raw ETL may contain
unrelated activity and identifiers. Keep it local and delete it after analysis.

The report counts providers and event IDs across the whole ETL. It then uses
XML timestamps with explicit UTC conversion to select the query window.
The WMI section seeds correlation from the payload client PID, then follows
activity and related activity IDs among in-window WMI records. The ETW header
PID identifies the emitting process and is shown separately. Operation and
namespace values are allowlisted. Raw serials and free-form messages are omitted.

In the reported capture, event 11 linked an `IWbemServices::Connect` operation
in `root/cimv2` to the query client PID. The linked debug records did not
identify the PhysicalDisk serial acquisition. Nearby StorPort events cannot
establish that their responses supplied SerialNumber. An empty ETL, a populated
ETL without in-window records, and correlated WMI with unresolved serial
acquisition are distinct results.

The narrowest additional evidence would be a provider-side observation at
property population that identifies either reuse of a stored value or the
specific lower result consumed. That would require provider diagnostics or
an authorized, build-specific debugger session with suitable symbols. A boot
capture is needed if population precedes the query. Kernel filter logging
would see only requests reaching its attached stacks and could still miss the
assignment. This workflow requires neither a filter change nor a restart.

[MSFT_PhysicalDisk](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/msft-physicaldisk) · [StorNVMe translation](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/stornvme-scsi-translation-support) · [Logman trace options](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/logman-create-trace)

### Inspect a candidate Spaceport detailed-drive response offline

The supplied investigation links the observed property to MISpace and its
input record. It suggests that IOCTL `0x00E70408` on Spaceport returns that
record. Neither the provider consumer nor the candidate kernel routine
establishes the response layout by itself.

The `storage_query::spaceport::candidate_serial_matches` library function
tests exactly one interpretation of a previously captured output. It checks
that capture metadata identifies a successful Spaceport request with that
code, and that the completed length fits both the caller's output capacity
and the available capture. It interprets bytes at `0xA3C` as a little-endian
32-bit byte offset from the start of output, requiring an even offset past
the field. It compares a bounded, NUL-terminated UTF-16LE candidate with
separately observed text. It returns only a match or a structural error.
These rules describe the tested hypothesis and synthetic fixtures. A match
does not authenticate the caller-supplied metadata, identify a physical disk,
or establish how much memory belongs exclusively to the string.

For Windows verification, run the existing read-only PhysicalDisk ETW
workflow above on the affected boot to look for a correlated request. If it
does not expose response bytes and request identity, use an authorized
debugger capture of the provider's **existing** request and returned buffer.
Do not issue the undocumented IOCTL as part of this procedure. Record the OS,
Spaceport and MISpace versions, the handle target, request parameters, status,
returned length and output capacity. Keep the bytes local. Feed copies into
the offline checker with the observed grouped serial as the expected text.
A second disk with a distinct identity is needed to show that the same rule
selects its corresponding record. Compare the namespace ID before and after
the read-only observation exactly, including punctuation.

Before a handler could write anything, the provider-side evidence needs to
show which base and units the consumer adds to the field, what bounds the
record and its strings, which versions support that interpretation, whether
fields share storage, and how to identify the selected disk independently of
its serial. A terminator does not establish writable capacity. Test evidence
for short output and incomplete enumeration is also needed. The current
offline checker returns no writable region and adds no kernel request tag.

The driver stores a per-attachment disk index. Spaceport and RaidPort use
index zero, so the index alone cannot identify Spaceport or PhysicalDrive0.
The IOCTL's low method bits encode METHOD_BUFFERED. A future route for this
request would need an explicit Spaceport attachment kind and a SystemBuffer
view bounded by the completed length and output capacity, with no MDL
fallback. Kernel completion work would need to be nonpageable and safe at
DISPATCH_LEVEL, preserve pending propagation, and participate in reliable
unload rundown. The existing unload path stops spinning after a fixed bound
even if its completion count is nonzero. This diagnostic does not add any
completion work.

[Windows IOCTL buffer methods](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/buffer-descriptions-for-i-o-control-codes)


### Opt-in Spaceport detailed-drive IOCTL counters (kernel debugger)

Build the Windows filter with `--features attachment-diagnostics` to enable a
fixed, debugger-readable `SmbiosSpaceportIoctlSnapshot` alongside the existing
13-row attachment snapshot. The normal driver build omits these counters.
This opt-in code registers a completion for an *existing* `0x00E70408`
request only when the request reaches the explicitly marked
`\\Device\\Spaceport` filter attachment **and the caller provides a nonzero
output capacity**. It never reads or changes input/output contents or any
hardware identifier, and it does not issue IOCTLs of its own.

In a live kernel debugging session on the affected boot, after confirming the
filter's Spaceport attachment succeeded, run `Get-PhysicalDisk` once and read:

```text
x *!SmbiosSpaceportIoctlSnapshot
dq <address-reported-by-x> L5
```

The five 64-bit fields, in order, are:

| Byte offset | Field | Meaning |
| --- | --- | --- |
| 0x00 | requests | Requests that registered an opt-in completion |
| 0x08 | completions | Invoked completion callbacks, regardless of status |
| 0x10 | successful_completions | Completion status with nonnegative NTSTATUS |
| 0x18 | last_status | Last completion NTSTATUS encoded as unsigned low 32 bits |
| 0x20 | last_information | Last `IoStatus.Information` value, in bytes |

These are global, monotonic counters since driver load, not per-process or
per-disk data. Values may change while they are read. A zero count does **not**
rule out Storage Spaces activity: request completion may be served before the
filter attaches, via another device stack or cache, or a different query path.
A count greater than zero proves only that the observed numeric IOCTL reached
the marked attachment; it does not authenticate the response layout, identify
which disk was queried, or prove the candidate offset `0xA3C` is correct.

**Operational warning:** this is a diagnostic kernel build and has not been
validated on live Windows hardware. Do not deploy automatically. The existing
unload implementation uses a bounded wait for outstanding completion callbacks;
this diagnostic adds one possible callback path but does not fix unload rundown.
Avoid unloading/reloading the diagnostic driver while requests are in flight.
No serial or response bytes are logged or exported by this feature.


### Read-only candidate string shape check (optional diagnostic build)

A separate `SmbiosSpaceportCandidateSnapshot` is emitted when the
`attachment-diagnostics` feature is enabled. On each completed Spaceport
`0x00E70408` request, the driver checks the existing output only if
`NTSTATUS == STATUS_SUCCESS`, the output capacity is nonzero, the completed
length is nonzero and does not exceed capacity, and `SystemBuffer` is non-null.
The IOCTL is METHOD_BUFFERED; this diagnostic never falls back to an MDL,
modifies response memory, or generates its own request.

The tested **unverified hypothesis** is a little-endian u32 byte offset at
output byte `0xA3C`, relative to the output start, pointing forward to an
even-aligned, NUL-terminated, well-formed UTF-16LE candidate of at most 256
non-NUL code units within the completed output. The check does **not**
compare the candidate text to a known serial and cannot establish semantic
ownership or writable capacity. A structural pass alone is not proof of the
Spaceport wire format.

Read the diagnostic snapshot on a running matching build:

```text
x *!SmbiosSpaceportCandidateSnapshot
dq <reported-address> L5
```

| QWORD | Meaning |
| --- | --- |
| 0 | Total checks (one per tagged completion) |
| 1 | Structurally plausible candidates |
| 2 | Last result code |
| 3 | Last candidate offset in bytes (untrusted) |
| 4 | Last non-NUL UTF-16 units scanned |

Last result codes: `1` plausible shape; `2` unsuccessful/nonzero status;
`3` zero or oversized completed length; `4` missing offset field;
`5` invalid/unaligned/out-of-range offset; `6` no terminator in bounds;
`7` malformed UTF-16; `8` null SystemBuffer. The recorded values may
reflect another request by the time WinDbg reads them. No serial bytes or
identity values are logged, and `UniqueId`/EUI are untouched.

For standalone tests without attempting to execute the `no_std` kernel
driver as a user-mode program:

```powershell
rustc --edition=2024 --test driver/src/spaceport_candidate.rs -o "$env:TEMP\spaceport_candidate_tests.exe"
& "$env:TEMP\spaceport_candidate_tests.exe" --nocapture
```

Build via `cargo build --release --manifest-path driver/Cargo.toml
--target x86_64-pc-windows-msvc --features attachment-diagnostics`.
Deploy only after reviewing the impact of running another completion callback
in the existing bounded unload/rundown implementation.
