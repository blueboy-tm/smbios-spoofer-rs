$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../tools/Trace-PhysicalDisk.ps1"

$row = ConvertTo-TraceEvent ([xml]@'
<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><Provider Name="Synthetic"/><EventID>42</EventID>
<TimeCreated SystemTime="2026-11-21T16:35:17+03:00"/><Execution ProcessID="123"/><Correlation ActivityID="a" RelatedActivityID="b"/></System>
<EventData><Data Name="Ioctl">private-value</Data><Data>anonymous</Data></EventData>
</Event>
'@)
if ($row.Provider -ne 'Synthetic' -or $row.Id -ne '42' -or $row.FieldNames -ne 'Ioctl' -or
    $row.ActivityId -ne 'a' -or $row.RelatedActivityId -ne 'b' -or $row.HeaderProcessId -ne '123') {
    throw 'Event metadata was not preserved.'
}
if (($row | ConvertTo-Json) -match 'private-value|anonymous') { throw 'Unexpected payload.' }
$row = ConvertTo-TraceEvent ([xml]'<Event><System><EventID>1</EventID><TimeCreated SystemTime="2026-11-21T13:35:17Z"/></System></Event>')
if ($row.FieldNames -or $row.ActivityId) { throw 'Missing fields were synthesized.' }

function logman.exe {
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'stop') { $script:StoppedSession = $args[1] }
    if ($args[0] -eq 'create') {
        if ($args -contains '-rf' -or $args -notcontains '-ets' -or $args -notcontains '-max') {
            throw 'Invalid logman arguments.'
        }
        $script:StartedSession = $args[2]
    }
}
function Start-Job { throw 'Synthetic query startup failure' }

try {
    Invoke-PhysicalDiskTrace
    throw 'Expected a query startup failure.'
}
catch {
    if ($_.Exception.Message -ne 'Synthetic query startup failure') { throw }
}
if (!$StartedSession -or $StartedSession -ne $StoppedSession) { throw 'Trace was not cleaned up.' }
if (Test-Path (Join-Path ([IO.Path]::GetTempPath()) "$StartedSession.etl.providers")) {
    throw 'Provider list was not cleaned up.'
}
Write-Output 'Event metadata and failure cleanup tests passed.'

$xml = @'
<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><Provider Name="Microsoft-Windows-WMI-Activity"/><EventID>11</EventID>
<TimeCreated SystemTime="2026-11-21T16:35:17+03:00"/><Execution ProcessID="123"/>
<Correlation ActivityID="a"/></System>
<UserData><Operation_New xmlns="urn:test"><ClientProcessId>17684</ClientProcessId>
<Operation>IWbemServices::Connect</Operation><NamespaceName>root/cimv2</NamespaceName>
<User>secret-user</User></Operation_New></UserData></Event>
'@
$row = ConvertTo-TraceEvent ([xml]$xml)
if ($row.Utc.ToString('HH:mm:ss zzz') -ne '13:35:17 +00:00' -or
    $row.ClientProcessId -ne 17684 -or $row.Operation -ne 'Connect' -or $row.Namespace -ne 'root/cimv2' -or
    ($row | ConvertTo-Json) -match 'secret-user') { throw 'UserData normalization failed.' }
$linked = ConvertTo-TraceEvent ([xml]($xml.Replace('ActivityID="a"', 'ActivityID="b" RelatedActivityID="a"').Replace('17684', '0')))
$unrelated = ConvertTo-TraceEvent ([xml]($xml.Replace('ActivityID="a"', 'ActivityID="c"').Replace('17684', '0').Replace('ProcessID="123"', 'ProcessID="17684"')))
if (@(Get-CorrelatedWmiEvent @($linked, $unrelated, $row) 17684).Count -ne 2) { throw 'PID/activity correlation failed.' }

function Get-WinEvent {
    if (!$script:MockEvent) { Write-Error 'Empty' -ErrorId NoMatchingEventsFound -ErrorAction Stop }
    else { $script:MockEvent }
}
$begin = [datetimeoffset]'2026-11-21T13:35:18Z'
$end = $begin.AddSeconds(1)
$empty = Read-PhysicalDiskTrace 'fake.etl' $begin $end 17684 6>&1 | Out-String
if ($empty -notmatch 'ETL is empty') { throw 'Empty ETL classification failed.' }
$script:MockEvent = [pscustomobject]@{ ProviderName = $row.Provider; Id = 11 }
$script:MockEvent | Add-Member ScriptMethod ToXml { $xml }
$script:MockEvent | Add-Member ScriptMethod Dispose { }
$unmatched = Read-PhysicalDiskTrace 'fake.etl' $begin $end 17684 6>&1 | Out-String
if ($unmatched -notmatch 'None decoded within the query window') { throw 'Unmatched ETL classification failed.' }
'Event time, UserData, correlation, and ETL state tests passed.'
