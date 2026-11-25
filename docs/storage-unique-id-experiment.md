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
git fetch origin feat/guarded-storage-unique-id-patch
git switch --detach FETCH_HEAD
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

## Kernel debugger counters: why the new ID is not visible

When built with `namespace-id-patch`, the driver now exports a fixed-layout
`SmbiosNamespaceIdCounters` symbol, visible to a **kernel debugger with
matching symbols**. It is read-only telemetry; no disk ID or output content
is retained, and no new user-mode IOCTL/device access is added.

The structure holds **18 consecutive 64-bit words** (144 bytes): first nine
for PropertyId 2 (`id`), then nine for PropertyId 3 (`duid`). Each group:

| Word offset | Field | Meaning |
|---|---|---|
| +0 | requests | Matching IRP reached our dispatch and had output capacity |
| +1 | completions | Our completion routine ran |
| +2 | successful | NTSTATUS nonnegative |
| +3 | failed | NTSTATUS negative |
| +4 | invalid_output | Successful response with zero/oversized length or null SystemBuffer |
| +5 | patch_attempts | Valid-length completion processed |
| +6 | patched | Actual **identifier bytes** changed (not merely DUID serial) |
| +7 | rejected | No matching old identifier, malformed descriptor or unavailable buffer |
| +8 | last_status | Last 32-bit NTSTATUS, zero-extended |

**Example from a kernel debugger** (with symbol loading configured for this
versioned driver; no command below writes to memory):

```text
x *!SmbiosNamespaceIdCounters
dq <address-returned-by-x> L12
```

WinDbg's `L12` uses hexadecimal count syntax: `0x12 = 18` qwords.
The exact module name may be the versioned `smbios-disk-filter-r....sys`
image, so first find the symbol using the `x *!...` wildcard. If the
symbol is unavailable, **do not guess a kernel address**. Match the PE/PDB
symbols for the loaded image or use a debugger-supported module listing.
Kernel debugging may require separate Windows debugging setup; ordinary
administrator PowerShell alone cannot directly read these kernel statics.

With the debugger attached, capture counters, trigger:

```powershell
cargo run --release --manifest-path tools/storage-query/Cargo.toml --target x86_64-pc-windows-msvc
```

Then capture counters again. Interpret **deltas**, not totals since boot:

- `requests = 0`: direct query did not traverse the instrumented filter path.
- `requests > 0`, `completions = 0`: completion not reached yet, or a path issue.
- `failed > 0` or `invalid_output > 0`: lower-layer status/length/buffer issue.
- `patch_attempts > 0`, `rejected > 0`: observed response did not pass exact guards,
  or no patchable buffer was available.
- `patched > 0` and direct tool still shows the old ID: another layer may be
  overwriting it or a different path is read by the tool.

**No inference about Get-Disk/Get-PhysicalDisk is justified by these counters
alone.** Keep PR #18 unmerged until the Windows build and diagnostic readout
have been checked.

## Reported Windows results for 277fa62

The configured image was signed and the service was running. The report
does not identify the loaded build or confirm its feature flags. The profile
and observed serial after the run agreed. Both Get-Disk and the status
report's PhysicalDisk row still showed the original EUI string. The two
direct IOCTL values were not collected in that post-run report.

On the same boot, collect the direct queries with the storage-query command
above and read both Get-Disk and Get-PhysicalDisk. Correlate disk number,
physical location and size independently of the reported serial and UID.
When available, record the loaded module and counter deltas around the
direct query. If both direct IDs change while PhysicalDisk does not, evidence
of the field actually consumed by MISpace is required before any further
handler. No Spaceport UID layout has been established.
