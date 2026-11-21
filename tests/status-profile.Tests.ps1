#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

# Parse the installer without executing its entry point or any Windows commands.
$installer = Join-Path (Split-Path -Parent $PSScriptRoot) 'install.ps1'
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) {
    throw ($parseErrors -join "`n")
}
foreach ($name in @('Get-V9ProfileValidation', 'Show-EspProfileStatus', 'Show-DriverServiceStatus', 'Write-Step', 'Write-Ok', 'Write-Warn')) {
    $definition = $ast.Find({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }, $false)
    if (-not $definition) { throw "Missing function $name in install.ps1" }
    . ([scriptblock]::Create($definition.Extent.Text))
}

function Get-V9Fixture([string]$Serial = 'ABCD_1234-56.  ', [byte]$Version = 9, [byte]$MagicFirstByte = 0x49) {
    $data = New-Object byte[] 1710
    # Little-endian SMBI magic, version, and a UUID that exposes byte-order mistakes.
    [byte[]]$header = @($MagicFirstByte, 0x42, 0x4D, 0x53, $Version, 0, 0, 0) + (1..16)
    $header.CopyTo($data, 0)
    $data[1705] = 0xFF # Include the last byte before the checksum.
    [System.Text.Encoding]::Unicode.GetBytes($Serial).CopyTo($data, 1574)
    $sum = ($data[0..1705] | Measure-Object -Sum).Sum
    [BitConverter]::GetBytes([uint32]$sum).CopyTo($data, 1706)
    return ,$data
}

function Confirm-InvalidProfile([byte[]]$Data, [string]$ReasonPattern) {
    $result = Get-V9ProfileValidation -Bytes $Data
    if ($result.Valid -or $result.Reason -notmatch $ReasonPattern) {
        throw "Expected an invalid profile with reason '$ReasonPattern'. Got '$($result.Reason)'."
    }
}

$valid = Get-V9Fixture
$result = Get-V9ProfileValidation -Bytes $valid
if (-not $result.Valid -or $result.Version -ne 9 -or
    $result.UUID -cne '01020304-0506-0708-090A-0B0C0D0E0F10' -or
    $result.DiskSerial -cne 'ABCD_1234-56.  ') {
    throw 'Expected the valid v9 fixture and its saved UUID and disk serial.'
}

Confirm-InvalidProfile $valid[0..23] 'Expected 1710 bytes, found 24'
Confirm-InvalidProfile $valid[0..1708] 'Expected 1710 bytes, found 1709'
Confirm-InvalidProfile ($valid + 0) 'Expected 1710 bytes, found 1711'

Confirm-InvalidProfile (Get-V9Fixture -MagicFirstByte 0) 'Expected magic'

foreach ($version in @(8, 10)) {
    Confirm-InvalidProfile (Get-V9Fixture -Version $version) "Expected version 9, found version $version"
}

$corrupt = Get-V9Fixture
$corrupt[1705] = 0
Confirm-InvalidProfile $corrupt 'Checksum mismatch'

foreach ($serial in @('', ';;', 'AB-3', (' ' * 20 + 'ABCD'), ('ABCD' + [char]0x00E9))) {
    Confirm-InvalidProfile (Get-V9Fixture -Serial $serial) 'Disk serial'
}
Confirm-InvalidProfile (Get-V9Fixture -Serial ('A' * 64)) 'missing its NUL terminator'

$testFile = [System.IO.Path]::GetTempFileName()
try {
    [System.IO.File]::WriteAllBytes($testFile, $valid)
    $output = Show-EspProfileStatus -Path $testFile 6>&1 | Out-String
    if ($output -notmatch 'ESP uuid\.dat valid' -or $output -notmatch 'Profile Disk Serial') {
        throw 'Expected validated profile values in status output.'
    }
    [System.IO.File]::WriteAllBytes($testFile, $corrupt)
    $output = Show-EspProfileStatus -Path $testFile 6>&1 | Out-String
    if ($output -notmatch 'ESP uuid\.dat invalid.+Checksum mismatch' -or $output -match 'Profile Disk Serial') {
        throw 'Expected a checksum diagnostic with profile values withheld.'
    }
} finally {
    Remove-Item -LiteralPath $testFile
}

# Mock the service lookup so this test also runs without Windows or a driver.
function Get-CimInstance {
    [CmdletBinding()]
    param([string]$ClassName, [string]$Filter)
    if ($ClassName -ne 'Win32_SystemDriver' -or $Filter -ne "Name = 'SmbiosDiskFilter'") {
        throw 'Expected a scoped system driver service query.'
    }
    [pscustomobject]@{ Name = 'SmbiosDiskFilter'; State = 'Running' }
}

$DriverServiceName = 'SmbiosDiskFilter'
$output = Show-DriverServiceStatus 6>&1 | Out-String
if ($output -notmatch "$DriverServiceName : Running" -or $output -notmatch 'IOCTL interception requires a separate test') {
    throw 'Expected the service state and an explicit distinction from IOCTL verification.'
}

'Status profile tests passed.'
