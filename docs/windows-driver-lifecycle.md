# Windows driver cleanup and clean reinstall

All commands are run from the repository directory in **Administrator PowerShell**
unless explicitly marked as read-only. The same modes are available from the
`install.bat` menu. Never run a kernel driver `.exe` directly in PowerShell.

## What is installed

The UEFI application lives on the EFI System Partition. The Windows driver is
registered under service **SmbiosDiskFilter** (SYSTEM_START). The installer
builds a Windows WDM image, signs a staged copy with a usable local
code-signing certificate, then copies it to an **immutable versioned filename**
under `%SystemRoot%\System32\drivers`, e.g.
`smbios-disk-filter-r<16-hex-digest>.sys`. It then points the service to the
versioned image **for the next boot**. It never overwrites a running driver
image, calls `sc stop`, or hot-reloads the kernel filter.

The default driver feature for `-Mode install` is
`spaceport-serial-patch`, which includes `attachment-diagnostics`.
**This patch is an experiment tailored to the previously observed Micron 2400
response and its exact original 40-character string.** It makes no claim to
support other drives, and it does not intentionally change NVMe EUI/NGUID or
Windows UniqueId. For general inspection without this experimental rewrite:

```powershell
.\install.ps1 -Mode install -DriverFeature attachment-diagnostics
# or -DriverFeature none
```

## Clean up all known project test drivers (preserve UEFI and v9 profile)

1. **Verify syntax, build and audit before removing the working driver**:
   ```powershell
   .\tools\test-installer-syntax.ps1
   .\install.ps1 -Mode build-only
   ```
   Then audit the current installation (read-only):
   ```powershell
   .\install.ps1 -Mode audit-drivers
   ```
2. **Disable and unregister just this project's driver service, then remove
   safely deletable project driver files**:
   ```powershell
   .\install.ps1 -Mode cleanup-drivers
   ```
   If the service is RUNNING, it preserves *all* driver images until the reboot.
   This targets the **SmbiosDiskFilter** service and known filenames
   `smbios-disk-filter.sys`, `smbios-disk-filter-diag.sys`,
   `smbios-disk-filter-diag.next.sys`,
   `smbios-disk-filter-diag.backup.sys`,
   `smbios-disk-filter-candidate.sys`,
   `smbios-disk-filter-patched.sys`, plus installer-created
   `smbios-disk-filter-r<16 hex>.sys`. It does **not** enumerate and delete
   third-party drivers.
3. **Restart Windows normally**. Do not stop the running driver manually.
4. **Run cleanup again** to remove the formerly loaded image, now that it has
   unloaded at boot:
   ```powershell
   .\install.ps1 -Mode cleanup-drivers
   .\install.ps1 -Mode audit-drivers
   ```
   If any file is still locked, the script reports it rather than claiming
   success. Investigate unexpected remaining services/images before proceeding.

This mode preserves `EFI\SmbiosSpoofer\uuid.dat`, the EFI boot entry, GPT
partition IDs, Windows volume IDs, and installed signing certificates. It
does not revert registry identifiers previously changed by older installers
(e.g., MachineGuid), because the original values cannot be reconstructed
safely without a backup. The new installer does **not** write these registry
identifiers.

## Fresh full installation

```powershell
.\install.ps1 -Mode install
```

This runs host Rust tests with an explicit Windows target, plus the standalone
driver unit tests; checks/builds the selected driver with Clippy; builds the
UEFI app; signs and stages the new WDM driver; and updates/reuses the UEFI
boot entry. A failure is fatal; it must not silently reuse an older driver.
A compatible driver-signing boot policy is required on the Windows test system.

**Reboot**, then verify from the same installer and Windows:

```powershell
.\install.ps1 -Mode status
sc.exe qc SmbiosDiskFilter
sc.exe query SmbiosDiskFilter
Get-Disk | Select-Object Number,SerialNumber,UniqueId
Get-PhysicalDisk | Select-Object FriendlyName,SerialNumber,UniqueId
Get-CimInstance Win32_DiskDrive | Select-Object Model,SerialNumber
```

Compare `UniqueId` with your recorded baseline. `RUNNING` by itself
does not prove that the loaded image is the newly configured one. If kernel
debugging is in use, use `!drvobj` and the driver section name to confirm
the loaded image before interpreting any snapshot.

Other modes:

- `-Mode build-only`: Build UEFI and driver with tests, without installing.
- `-Mode install-driver`: Build and stage only the Windows driver; preserves EFI.
- `-Mode uninstall-driver`: Same two-phase cleanup as `cleanup-drivers`.
- `-Mode uninstall`: Also removes the UEFI spoofer's boot entry and its ESP
  directory (including the ESP v9 profile). Back up the profile first.
- `-SkipTests`: Explicitly bypass tests, not recommended for a clean install.

## Syntax-only checks

Use the built-in Windows PowerShell 5.1 parser to reject syntax errors without
running the installer:

```powershell
.\tools\test-installer-syntax.ps1
```

PowerShell's parser cannot prove that a kernel driver is safe to unload, that
the selected certificate passes Windows kernel policy, or that the UEFI boot
entry was configured successfully. Testing on the actual Windows machine
remains necessary.
