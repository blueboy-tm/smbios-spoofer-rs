$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../tools/Trace-PhysicalDisk.ps1"

$row = ConvertTo-TraceEvent ([xml]@'
<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><Provider Name="Synthetic"/><EventID>42</EventID>
<Execution ProcessID="123"/><Correlation ActivityID="a" RelatedActivityID="b"/></System>
<EventData><Data Name="Ioctl">private-value</Data><Data>anonymous</Data></EventData>
</Event>
'@)
if ($row.Provider -ne 'Synthetic' -or $row.Id -ne '42' -or $row.FieldNames -ne 'Ioctl' -or
    $row.ActivityId -ne 'a' -or $row.RelatedActivityId -ne 'b' -or $row.ProcessId -ne '123') {
    throw 'Event metadata was not preserved.'
}
if (($row | ConvertTo-Json) -match 'private-value|anonymous') { throw 'Unexpected payload.' }
$row = ConvertTo-TraceEvent ([xml]'<Event><System><EventID>1</EventID></System></Event>')
if ($row.FieldNames -or $row.ActivityId) { throw 'Missing fields were synthesized.' }

function logman.exe {
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'stop') { $script:StoppedSession = $args[1] }
    if ($args[0] -eq 'create') { $script:StartedSession = $args[2] }
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
