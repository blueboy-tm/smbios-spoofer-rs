#Requires -Version 5.1
# Driver lifecycle for install.ps1. Dot-sourced after the shared helpers are defined.
# All file operations are limited to this project's System32\drivers artifacts.

$DriverDirectory = Join-Path $env:SystemRoot "System32\drivers"

function Get-ConfiguredSpooferDriverPath {
    $key = "HKLM:\SYSTEM\CurrentControlSet\Services\$DriverServiceName"
    if (-not (Test-Path -LiteralPath $key)) { return $null }
    $raw = [string](Get-ItemProperty -LiteralPath $key -Name ImagePath -ErrorAction SilentlyContinue).ImagePath
    if (-not $raw) { return $null }
    $raw = [Environment]::ExpandEnvironmentVariables($raw.Trim('"'))
    $raw = $raw -replace '^\\\?\?\\', ''
    # Accept the two Service Control Manager path forms and absolute paths
    # only if they resolve to System32\drivers. Never follow arbitrary paths.
    $root = [IO.Path]::GetFullPath($DriverDirectory).TrimEnd('\') + '\'
    if ($raw -match '^\\SystemRoot\\(.+)$') {
        $path = Join-Path $env:SystemRoot $Matches[1]
    } elseif ($raw -match '^System32\\drivers\\(.+)$') {
        $path = Join-Path $DriverDirectory $Matches[1]
    } elseif ([IO.Path]::IsPathRooted($raw)) {
        $path = $raw
    } else {
        return $null
    }
    $full = [IO.Path]::GetFullPath($path)
    if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) { return $null }
    $name = [IO.Path]::GetFileName($full)
    if ($name -notmatch '^smbios-disk-filter[\w.-]*\.sys$') { return $null }
    return $full
}

function Get-SpooferDriverArtifacts {
    $legacy = @(
        "smbios-disk-filter.sys",
        "smbios-disk-filter-diag.sys",
        "smbios-disk-filter-diag.next.sys",
        "smbios-disk-filter-diag.backup.sys",
        "smbios-disk-filter-candidate.sys",
        "smbios-disk-filter-patched.sys"
    )
    foreach ($name in $legacy) {
        $path = Join-Path $DriverDirectory $name
        if (Test-Path -LiteralPath $path -PathType Leaf) { $path }
    }
    # Only installer-generated immutable versioned artifacts; no broad *.sys deletion.
    foreach ($file in @(Get-ChildItem -LiteralPath $DriverDirectory -File -Filter "smbios-disk-filter-r*.sys" -ErrorAction SilentlyContinue)) {
        if ($file.Name -match '^smbios-disk-filter-r[0-9a-f]{16}\.sys$') { $file.FullName }
    }
    $configured = Get-ConfiguredSpooferDriverPath
    if ($configured -and (Test-Path -LiteralPath $configured -PathType Leaf)) { $configured }
}

function Show-SpooferDriverInventory {
    Write-Step "Project driver files in System32\drivers:"
    $configured = Get-ConfiguredSpooferDriverPath
    $paths = @(Get-SpooferDriverArtifacts | Sort-Object -Unique)
    if ($paths.Count -eq 0) {
        Write-Ok "No known SMBIOS disk filter files remain."
    } else {
        foreach ($path in $paths) {
            $signature = (Get-AuthenticodeSignature -FilePath $path).Status
            $role = if ($configured -and $path -ieq $configured) { "CONFIGURED" } else { "legacy/staged" }
            Write-Host ("    {0}  [{1}; signature={2}]" -f (Split-Path $path -Leaf), $role, $signature)
        }
    }
}

function Show-DriverServiceStatus {
    Write-Host ""
    Write-Step "SmbiosDiskFilter service and currently configured image"
    $service = Get-CimInstance Win32_SystemDriver -Filter "Name = '$DriverServiceName'" -ErrorAction SilentlyContinue
    if ($service) {
        Write-Host ("    Service: {0}; State: {1}; StartMode: {2}" -f $service.Name, $service.State, $service.StartMode)
    } else {
        Write-Warn "No $DriverServiceName service registered."
    }
    $configured = Get-ConfiguredSpooferDriverPath
    if ($configured) {
        Write-Host "    Configured image: $configured"
        if (Test-Path -LiteralPath $configured) {
            Write-Host ("    Signature: {0}" -f (Get-AuthenticodeSignature -FilePath $configured).Status)
        } else {
            Write-Warn "Configured driver image does not exist."
        }
    }
    Show-SpooferDriverInventory
    Write-Host ""
    Write-Step "Disk identity consistency (read-only)"
    try {
        Get-PhysicalDisk -ErrorAction Stop |
            Select-Object FriendlyName, SerialNumber, UniqueId |
            Format-Table -AutoSize | Out-Host
    } catch {
        Write-Warn "Get-PhysicalDisk unavailable: $($_.Exception.Message)"
    }
    Write-Host "    A RUNNING service does not prove the newly configured image is already loaded."
}

function Test-UsableDriverSigningCertificate($certificate) {
    if (-not $certificate -or -not $certificate.HasPrivateKey) { return $false }
    $rsa = $null
    try {
        $rsa = [System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($certificate)
        if ($null -eq $rsa) { return $false }
        $proof = $rsa.SignData([byte[]](1,2,3,4), [System.Security.Cryptography.HashAlgorithmName]::SHA256, [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
        return $proof.Length -gt 0
    } catch { return $false }
    finally { if ($null -ne $rsa) { $rsa.Dispose() } }
}

function Get-SpooferSigningCertificate {
    # Prefer the certificate which signed the working driver on this machine.
    $configured = Get-ConfiguredSpooferDriverPath
    if ($configured -and (Test-Path -LiteralPath $configured)) {
        $signer = (Get-AuthenticodeSignature -FilePath $configured).SignerCertificate
        if ($signer) {
            $local = Get-Item "Cert:\LocalMachine\My\$($signer.Thumbprint)" -ErrorAction SilentlyContinue
            if (Test-UsableDriverSigningCertificate $local) { return $local }
        }
    }
    $subjects = @("CN=SmbiosSpooferDiagnosticCert", "CN=SmbiosSpooferKernelCert")
    foreach ($candidate in @(Get-ChildItem Cert:\LocalMachine\My -CodeSigningCert -ErrorAction SilentlyContinue | Sort-Object NotBefore -Descending)) {
        if (($subjects -contains $candidate.Subject) -and (Test-UsableDriverSigningCertificate $candidate)) {
            return $candidate
        }
    }
    Write-Step "Creating a code-signing certificate for this project..."
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=SmbiosSpooferKernelCert" -Provider "Microsoft Software Key Storage Provider" -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -CertStoreLocation "Cert:\LocalMachine\My" -NotAfter (Get-Date).AddYears(5) -ErrorAction Stop
    if (-not (Test-UsableDriverSigningCertificate $cert)) { throw "New certificate has no working private key." }
    return $cert
}

function Get-SignToolPath {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    $sdkRoot = Join-Path ([Environment]::GetFolderPath("ProgramFilesX86")) "Windows Kits\10\bin"
    if (Test-Path -LiteralPath $sdkRoot) {
        $candidate = Get-ChildItem -LiteralPath $sdkRoot -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
            Sort-Object FullName -Descending | Select-Object -First 1
        if ($candidate) { return $candidate.FullName }
    }
    throw "Windows SDK x64 signtool.exe not found."
}

function Install-WindowsDiskFilterDriver {
    if (-not (Test-Path -LiteralPath $LocalDriverCopy -PathType Leaf)) {
        throw "No built driver in $LocalDriverCopy. Run .\install.ps1 -Mode build-only first."
    }
    Write-Step "Preparing versioned, signed kernel image; no in-place replacement of running drivers."
    $bcd = (& bcdedit /enum "{current}" 2>&1) | Out-String
    if ($LASTEXITCODE -ne 0) { throw "Unable to read Windows boot signing policy (bcdedit)." }
    if ($bcd -notmatch '(?im)^\s*testsigning\s+Yes\b') {
        Write-Warn "Enabling Windows test-signing policy for self-signed kernel driver; reboot required."
        & bcdedit /set "{current}" testsigning on
        if ($LASTEXITCODE -ne 0) { throw "Cannot enable testsigning (e.g. Secure Boot policy). Driver installation stopped." }
    }
    $sourceHash = (Get-FileHash -LiteralPath $LocalDriverCopy -Algorithm SHA256).Hash.ToLowerInvariant()
    $filename = "smbios-disk-filter-r$($sourceHash.Substring(0,16)).sys"
    $target = Join-Path $DriverDirectory $filename

    if (Test-Path -LiteralPath $target) {
        if ((Get-AuthenticodeSignature -FilePath $target).Status -ne "Valid") {
            throw "Existing versioned image has invalid signature: $target. Refusing to overwrite it."
        }
        Write-Ok "Reusing signed image: $target"
    } else {
        $certificate = Get-SpooferSigningCertificate
        foreach ($storeName in @("Root", "TrustedPublisher")) {
            $store = New-Object System.Security.Cryptography.X509Certificates.X509Store($storeName, "LocalMachine")
            try {
                $store.Open("ReadWrite")
                if (-not ($store.Certificates | Where-Object { $_.Thumbprint -eq $certificate.Thumbprint })) {
                    $store.Add($certificate)
                }
            } finally { $store.Close() }
        }
        $signTool = Get-SignToolPath
        $staged = Join-Path $env:TEMP ("smbios-filter-" + [guid]::NewGuid().ToString("N") + ".sys")
        try {
            Copy-Item -LiteralPath $LocalDriverCopy -Destination $staged -Force -ErrorAction Stop
            & $signTool sign /fd SHA256 /s My /sm /sha1 $certificate.Thumbprint $staged
            if ($LASTEXITCODE -ne 0) { throw "SignTool sign failed." }
            if ((Get-AuthenticodeSignature -FilePath $staged).Status -ne "Valid") {
                throw "Staged driver's Authenticode signature is invalid."
            }
            Copy-Item -LiteralPath $staged -Destination $target -ErrorAction Stop
            $expected = (Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash
            $actual = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
            if ($expected -ne $actual) { throw "Staged and installed hashes differ." }
            if ((Get-AuthenticodeSignature -FilePath $target).Status -ne "Valid") {
                throw "Installed driver signature validation failed."
            }
        } finally {
            Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
        }
        Write-Ok "Signed and installed immutable image: $target"
    }

    $imagePath = "System32\drivers\$filename"
    # Kernel driver services are reliably enumerated by Win32_SystemDriver;
    # Get-Service may omit device drivers on some Windows installations.
    $service = Get-CimInstance Win32_SystemDriver -Filter "Name = '$DriverServiceName'" -ErrorAction SilentlyContinue
    if ($service) {
        & sc.exe config $DriverServiceName type= kernel start= system error= ignore binPath= $imagePath
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to reconfigure $DriverServiceName. If previously deleted, reboot and rerun cleanup-drivers."
        }
    } else {
        & sc.exe create $DriverServiceName type= kernel start= system error= ignore binPath= $imagePath DisplayName= "SMBIOS Disk Serial Filter"
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to create $DriverServiceName (possibly pending deletion). Reboot, rerun cleanup-drivers, then install."
        }
    }
    Write-Ok "Configured $DriverServiceName for next boot: $imagePath"
    Write-Warn "Reboot required to load the new image. Do not Stop/Start a live kernel filter."
    Write-Host "    Test-signed kernel drivers also require a compatible Windows boot policy."
}

function Uninstall-WindowsDiskFilterDriver {
    # Never call sc stop: unload rundown for this filter is not yet proven.
    # Kernel driver services are reliably enumerated by Win32_SystemDriver;
    # Get-Service may omit device drivers on some Windows installations.
    $service = Get-CimInstance Win32_SystemDriver -Filter "Name = '$DriverServiceName'" -ErrorAction SilentlyContinue
    $wasRunning = $service -and $service.State -eq 'Running'

    if ($service) {
        & sc.exe config $DriverServiceName start= disabled
        if ($LASTEXITCODE -ne 0) { throw "Failed to disable $DriverServiceName." }
        & sc.exe delete $DriverServiceName
        if ($LASTEXITCODE -ne 0) { throw "Failed to delete service registration $DriverServiceName." }
        Write-Ok "Service disabled and marked for deletion. Running driver remains loaded until reboot."
    }

    $remaining = @()
    # Current ImagePath may already point at a *different* next-boot image.
    # Consequently we cannot identify the loaded image from SCM configuration.
    # When RUNNING, leave ALL project driver files intact until the reboot.
    foreach ($path in @(Get-SpooferDriverArtifacts | Sort-Object -Unique)) {
        if ($wasRunning) {
            $remaining += $path
            Write-Warn "Preserving image while filter is running: $path"
            continue
        }
        try {
            Remove-Item -LiteralPath $path -Force -ErrorAction Stop
            if (Test-Path -LiteralPath $path) { throw "File still exists." }
            Write-Ok "Removed: $path"
        } catch {
            $remaining += $path
            Write-Warn "Could not remove $path ($($_.Exception.Message)). Retry after reboot."
        }
    }
    if ($wasRunning) {
        Write-Warn "REBOOT REQUIRED. After reboot run .\install.ps1 -Mode cleanup-drivers again."
        return
    }
    if ($remaining.Count -gt 0) {
        throw "Cleanup incomplete: $($remaining.Count) driver file(s) could not be removed. Inspect the listed files."
    }
    Write-Ok "Driver cleanup complete. UEFI profile, firmware boot entry and signing certificates were preserved."
}
