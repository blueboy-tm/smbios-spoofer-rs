#Requires -Version 5.1
# Parse only. Does not execute install.ps1, touch services, or modify boot data.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$files = @(
    (Join-Path $root "install.ps1"),
    (Join-Path $PSScriptRoot "driver-lifecycle.ps1")
)
$failures = @()
foreach ($file in $files) {
    $tokens = $null
    $parseErrors = $null
    [System.Management.Automation.Language.Parser]::ParseFile(
        $file, [ref]$tokens, [ref]$parseErrors
    ) | Out-Null
    if ($null -ne $parseErrors -and $parseErrors.Count -gt 0) {
        foreach ($err in $parseErrors) {
            $failures += "$file : $($err.Extent.StartLineNumber) : $($err.Message)"
        }
    } else {
        Write-Host "Syntax OK: $file"
    }
}
if ($failures.Count -gt 0) {
    throw ("PowerShell syntax errors:" + [Environment]::NewLine + ($failures -join [Environment]::NewLine))
}
Write-Host "All installer PowerShell files parsed successfully."
