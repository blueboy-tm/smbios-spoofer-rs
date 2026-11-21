[CmdletBinding()]
param()

function ConvertTo-TraceEvent {
    param([xml]$EventXml)
    $system = $EventXml.Event.System
    $fields = @{}
    foreach ($node in $EventXml.SelectNodes(
        "/*[local-name()='Event']/*[local-name()='EventData']/*[local-name()='Data'][@Name] | /*[local-name()='Event']/*[local-name()='UserData']//*[not(*)]")) {
        $name = if ($node.LocalName -eq 'Data') { $node.GetAttribute('Name') } else { $node.LocalName }
        $fields[$name] = $node.InnerText
    }
    $client = 0
    $null = [int]::TryParse($fields['ClientProcessId'], [ref]$client)
    $operation = if ($fields['Operation'] -match '^(?:IWbemServices::)?(Connect|ExecQueryAsync|ExecQuery|ExecMethod|GetObject|CreateInstanceEnum)(?:$|\s)') {
        $Matches[1]
    }
    $namespace = switch (($fields['NamespaceName'] -replace '\\', '/')) {
        'root/cimv2' { 'root/cimv2' }
        'root/Microsoft/Windows/Storage' { 'root/Microsoft/Windows/Storage' }
    }
    [pscustomobject]@{
        Provider = $system.Provider.Name
        Id = [string]$system.EventID
        Utc = [datetimeoffset]::Parse($system.TimeCreated.SystemTime).ToUniversalTime()
        HeaderProcessId = $system.Execution.ProcessID
        ClientProcessId = $client
        ActivityId = $system.Correlation.ActivityID
        RelatedActivityId = $system.Correlation.RelatedActivityID
        Operation = $operation
        Namespace = $namespace
        FieldNames = @($fields.Keys | Sort-Object) -join ', '
    }
}

function Get-CorrelatedWmiEvent {
    param($Events, [int]$QueryProcessId)
    $ids = @{}
    $wmi = @($Events | Where-Object Provider -eq 'Microsoft-Windows-WMI-Activity')
    do {
        $previous = $ids.Count
        foreach ($event in $wmi) {
            $links = @(@($event.ActivityId, $event.RelatedActivityId) | Where-Object {
                $_ -and $_ -ne '{00000000-0000-0000-0000-000000000000}'
            })
            if ($event.ClientProcessId -eq $QueryProcessId -or
                ($event.ClientProcessId -eq 0 -and @($links | Where-Object { $ids.ContainsKey($_) }).Count)) {
                foreach ($id in $links) { $ids[$id] = $true }
            }
        }
    } while ($ids.Count -gt $previous)
    $wmi | Where-Object {
        $_.ClientProcessId -eq $QueryProcessId -or ($_.ClientProcessId -eq 0 -and
            (($_.ActivityId -and $ids.ContainsKey($_.ActivityId)) -or
             ($_.RelatedActivityId -and $ids.ContainsKey($_.RelatedActivityId))))
    }
}

function Read-PhysicalDiskTrace {
    param($Path, [datetimeoffset]$Begin, [datetimeoffset]$End, [int]$QueryProcessId)
    $counts = @{}
    $wmi = New-Object 'System.Collections.Generic.List[object]'
    $seen = 0
    $matched = 0
    $invalid = 0
    $truncated = $false
    try {
        Get-WinEvent -Path $Path -Oldest -ErrorAction Stop | ForEach-Object {
            $seen++
            $key = "$($_.ProviderName) / $($_.Id)"
            $counts[$key]++
            try {
                $event = ConvertTo-TraceEvent ([xml]$_.ToXml())
                if ($event.Utc -ge $Begin -and $event.Utc -le $End) {
                    $matched++
                    if ($event.Provider -eq 'Microsoft-Windows-WMI-Activity') {
                        if ($wmi.Count -lt 4096) { $wmi.Add($event) }
                        else { $truncated = $true }
                    }
                }
            }
            catch { $invalid++ }
            finally { $_.Dispose() }
        }
    }
    catch {
        if ($seen -ne 0 -or ($_.FullyQualifiedErrorId -split ',')[0] -ne 'NoMatchingEventsFound') {
            Write-Warning 'ETL enumeration failed. Counts may be incomplete.'
            return
        }
    }
    if ($seen -eq 0) { Write-Host 'ETL is empty (no readable event records).'; return }
    Write-Host 'Provider and event ID counts for the entire ETL'
    $counts.GetEnumerator() | Sort-Object Name | Format-Table Name, Value -AutoSize | Out-Host
    if ($invalid) { Write-Warning "$invalid records could not be decoded or timed." }
    if ($matched -eq 0) { Write-Host 'ETL contains events. None decoded within the query window.'; return }
    if ($truncated) { Write-Warning 'WMI correlation is limited to the first 4096 in-window WMI records.' }
    $correlated = @(Get-CorrelatedWmiEvent $wmi $QueryProcessId)
    if ($correlated.Count) {
        Write-Host 'WMI activity correlated by client PID or linked activity ID. Serial acquisition remains unresolved.'
        $correlated | Format-List | Out-Host
    }
    else { Write-Host 'No WMI activity could be correlated to the query. Serial acquisition remains unresolved.' }
    Write-Host 'Nearby StorPort events cannot establish which response supplied SerialNumber.'
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
        '-f', 'bincirc', '-max', '8')
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
        $job = Start-Job -ScriptBlock {
            [pscustomobject]@{ Utc = [datetimeoffset]::UtcNow.ToString('o'); QueryProcessId = $PID; Phase = 'Begin' }
            Get-PhysicalDisk -ErrorAction Stop | Out-Null
            [pscustomobject]@{ Utc = [datetimeoffset]::UtcNow.ToString('o'); QueryProcessId = $PID; Phase = 'End' }
        }
        if (-not (Wait-Job $job -Timeout 8)) { throw 'PhysicalDisk query exceeded eight seconds.' }
        $result = @(Receive-Job $job -ErrorAction Stop)
    }
    finally {
        $stopFailed = $false
        if ($started) {
            & logman.exe stop $session -ets | Out-Host
            if ($LASTEXITCODE -ne 0) {
                & logman.exe stop $session -ets | Out-Host
                if ($LASTEXITCODE -ne 0) {
                    Write-Warning "Check for a remaining session with: logman query $session -ets"
                    Write-Warning "If still active, stop only this session with: logman stop $session -ets"
                    $stopFailed = $true
                }
            }
        }
        if ($job) { Remove-Job $job -Force }
        Remove-Item -LiteralPath $providerFile -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $etl) { Write-Host "Local ETL: $etl" }
        if ($stopFailed) { throw 'ETW session stop failed twice.' }
    }

    if ($result.Count -ne 2 -or $result[0].Phase -ne 'Begin' -or $result[1].Phase -ne 'End') {
        throw 'Query did not return both timing markers.'
    }
    $result | Format-Table Phase, Utc, QueryProcessId -AutoSize | Out-Host
    Read-PhysicalDiskTrace $etl $result[0].Utc $result[1].Utc $result[0].QueryProcessId
}

if ($MyInvocation.InvocationName -ne '.') {
    Invoke-PhysicalDiskTrace
}
