param(
    [Parameter(Mandatory = $true)][string]$LiveLog,
    [int]$InitSeconds = 45,
    [int]$Every = 3,
    [int]$TotalSeconds = 240
)
$trigger = Join-Path $env:TEMP 'tombox_capture_now'
$capDir = Join-Path $env:TEMP 'tombox_rtcap'
$marks = Join-Path $env:TEMP 'tombox_capture_marks.txt'
Set-Content -Path $marks -Value '' -Encoding utf8
$start = Get-Date
$lastLines = 0
$nextInit = 0
$lastRequest = [datetime]::MinValue
function Write-Mark([string]$text) {
    Add-Content -Path $marks -Value ("{0:HH:mm:ss.fff}Z {1}" -f (Get-Date).ToUniversalTime(), $text) -Encoding utf8
}
function Request-Capture([string]$why) {
    $now = Get-Date
    if (($now - $lastRequest).TotalSeconds -lt 1.5) { return }
    if (Test-Path $trigger) {
        $age = ($now - (Get-Item $trigger).LastWriteTime).TotalSeconds
        if ($age -ge 1.5) { Write-Mark ("still-pending {0:N1}s: no present consumed the trigger | {1}" -f $age, $why) }
        return
    }
    $seq = (Get-ChildItem $capDir -Directory -ErrorAction SilentlyContinue | Measure-Object).Count
    Set-Content -Path $trigger -Value 'frames=1' -Encoding ascii
    $script:lastRequest = $now
    Write-Mark ("request dirs_before={0} | {1}" -f $seq, $why)
}
while (((Get-Date) - $start).TotalSeconds -lt $TotalSeconds) {
    $t = ((Get-Date) - $start).TotalSeconds
    if ($t -lt $InitSeconds -and $t -ge $nextInit) {
        Request-Capture ("init t={0:N0}s" -f $t)
        $nextInit += $Every
    }
    if (Test-Path $LiveLog) {
        $lines = @(Get-Content $LiveLog -ErrorAction SilentlyContinue)
        if ($lines.Count -gt $lastLines) {
            foreach ($l in $lines[$lastLines..($lines.Count - 1)]) {
                if ($l -match 'Loading screen showing|LogF5Player: .*IMediaPlayer::Open\(|PlaybackResumed: Valid Sound') {
                    Request-Capture ($l.Substring(0, [Math]::Min(160, $l.Length)))
                }
            }
            $lastLines = $lines.Count
        }
    }
    Start-Sleep -Milliseconds 250
}
if (Test-Path $trigger) { Remove-Item $trigger -Force }
