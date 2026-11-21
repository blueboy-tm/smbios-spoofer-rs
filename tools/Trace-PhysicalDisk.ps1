[CmdletBinding()]
param()

function ConvertTo-TraceEvent {
    param([xml]$EventXml)
    $system = $EventXml.Event.System
    $fields = @($EventXml.Event.EventData.Data | ForEach-Object {
        if ($_ -is [System.Xml.XmlElement]) { $_.GetAttribute('Name') }
    } | Where-Object { $_ })
    [pscustomobject]@{
        Provider = $system.Provider.Name
        Id = [string]$system.EventID
        Utc = $system.TimeCreated.SystemTime
        ProcessId = $system.Execution.ProcessID
        ActivityId = $system.Correlation.ActivityID
        RelatedActivityId = $system.Correlation.RelatedActivityID
        FieldNames = ($fields -join ', ')
    }
}

function Invoke-PhysicalDiskTrace {
    $providers = @(
        'Microsoft-Windows-WMI-Activity'
        'Microsoft-Windows-StorageManagement'
        'Microsoft-Windows-StorageSpaces-Driver'
        'Microsoft-Windows-StorPort'
        'Microsoft-Windows-StorNVMe'
    )
    $available = @(foreach ($provider in $providers) {
        $null = & logman.exe query providers $provider 2>&1
        if ($LASTEXITCODE -eq 0) { $provider }
    })
    if ($available.Count -eq 0) { throw 'No candidate ETW providers were available.' }
    Write-Host "Available providers: $($available -join ', ')"
    $session = 'PhysicalDisk-' + [guid]::NewGuid().ToString('N')
    $etl = Join-Path ([IO.Path]::GetTempPath()) "$session.etl"
    $arguments = @('create', 'trace', $session, '-ets', '-o', $etl,
        '-f', 'bincirc', '-max', '8', '-rf', '00:00:15')
    $providerFile = "$etl.providers"
    $arguments += @('-pf', $providerFile, '-bs', '64', '-nb', '16', '64')

    $job = $null
    $started = $false
    try {
        $available | ForEach-Object { "$_ 0xffffffffffffffff 4" } |
            Set-Content -LiteralPath $providerFile -Encoding Ascii -ErrorAction Stop
        & logman.exe @arguments | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'ETW start failed. Check trace permissions and provider availability.' }
        $started = $true
        $begin = [datetime]::UtcNow
        $job = Start-Job -ScriptBlock {
            [pscustomobject]@{ Utc = [datetime]::UtcNow; QueryProcessId = $PID; Phase = 'Begin' }
            Get-PhysicalDisk -ErrorAction Stop | Select-Object DeviceId, SerialNumber
            [pscustomobject]@{ Utc = [datetime]::UtcNow; QueryProcessId = $PID; Phase = 'End' }
        }
        if (-not (Wait-Job $job -Timeout 8)) { throw 'PhysicalDisk query exceeded eight seconds.' }
        $result = @(Receive-Job $job -ErrorAction Stop)
        $end = [datetime]::UtcNow
    }
    finally {
        if ($started) {
            & logman.exe stop $session -ets | Out-Host
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "Could not explicitly stop $session. The session was configured for 15 seconds."
            }
        }
        if ($job) { Remove-Job $job -Force }
        Remove-Item -LiteralPath $providerFile -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $etl) { Write-Host "Local ETL: $etl" }
    }

    $result
    $markers = @($result | Where-Object { $_.Phase -in @('Begin', 'End') })
    if ($markers.Count -eq 2) {
        $begin = $markers[0].Utc
        $end = $markers[1].Utc
    }
    try {
        Get-WinEvent -FilterHashtable @{ Path = $etl; StartTime = $begin; EndTime = $end } -Oldest -MaxEvents 200 -ErrorAction Stop |
            ForEach-Object {
                try { ConvertTo-TraceEvent ([xml]$_.ToXml()) }
                catch { Write-Warning 'An event schema could not be decoded.' }
                finally { $_.Dispose() }
            }
    }
    catch { Write-Warning "No usable event summary: $_" }
    Write-Host 'At most 200 events are displayed. Timing and field names alone cannot identify the source of SerialNumber. Missing events leave the source unresolved.'
}

if ($MyInvocation.InvocationName -ne '.') {
    Invoke-PhysicalDiskTrace
}
