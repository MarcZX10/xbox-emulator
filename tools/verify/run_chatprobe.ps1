param(
    [string]$Games = 'D:\xbox-pc-port\games',
    [string]$TargetDir = 'D:\xbox-pc-port\emulator\target-wf-3',
    [string]$Evidence = (Join-Path $PSScriptRoot 'evidence')
)
$ErrorActionPreference = 'Stop'
$probe = Join-Path $TargetDir 'release\tombox-probe.exe'
if (-not (Test-Path $probe)) { throw "build first: cargo build -p tombox-probe --release -j 2 (CARGO_TARGET_DIR=$TargetDir)" }
$scratch = Join-Path ([IO.Path]::GetTempPath()) 'tombox-probe-stage'
New-Item -ItemType Directory -Force $Evidence | Out-Null

$fn = Join-Path $Games 'Fortnite\extracted'
$fnBin = Join-Path $fn 'Mount\FortniteGame\Binaries\XSX'
$fnXvd = Join-Path $fn 'EmbeddedXvd'
$tm = Get-ChildItem (Join-Path $Games 'Trackmania\extracted') -Directory | Select-Object -First 1
$tmBin = Join-Path $tm.FullName 'Mount'
$tmXvd = Join-Path $tm.FullName 'EmbeddedXvd'
$tr = Get-ChildItem -LiteralPath (Join-Path $Games 'Terraria\extracted') -Directory | Select-Object -First 1
$trMount = Join-Path $tr.FullName 'Mount'
$trXvd = Join-Path $tr.FullName 'EmbeddedXvd'
$stage = @()
$dist = 'D:\xbox-pc-port\emulator\dist\tombox-compat-build2'
if (Test-Path $dist) { $stage = Get-ChildItem $dist -Filter *.dll | ForEach-Object { '--stage'; $_.FullName } }
$ahw = Get-ChildItem 'D:\xbox-pc-port\emulator' -Directory -Filter 'target*' | ForEach-Object { Join-Path $_.FullName 'release\AudioHALWrapper.dll' } | Where-Object { Test-Path $_ } | Get-Item | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($ahw) { $stage += @('--stage', $ahw.FullName) }

function Invoke-Probe([string[]]$a) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $probe @a 2>&1 | ForEach-Object { "$_" }
    $ErrorActionPreference = $old
    if ($LASTEXITCODE -ne 0) { throw "tombox-probe $($a[0]) failed ($LASTEXITCODE)" }
}

Invoke-Probe (@('load', '--title', 'Fortnite', '--bin-dir', $fnBin, '--dll', 'GameChat2.dll', '--xvd', $fnXvd, '--scratch', $scratch, '--expect-from', 'FortniteClient-XSX-Shipping.exe', '--out', (Join-Path $Evidence 'fortnite_gamechat2_load.json')) + $stage)
Invoke-Probe @('load', '--title', 'Fortnite', '--bin-dir', $fnBin, '--dll', 'MSVCP140.dll', '--xvd', $fnXvd, '--scratch', $scratch, '--out', (Join-Path $Evidence 'fortnite_msvcp140_load.json'))
Invoke-Probe (@('load', '--title', 'Trackmania', '--bin-dir', $tmBin, '--dll', 'GameChat2.dll', '--xvd', $tmXvd, '--scratch', $scratch, '--out', (Join-Path $Evidence 'trackmania_gamechat2_load.json')) + $stage)
Invoke-Probe @('load', '--title', 'Terraria', '--bin-dir', $trMount, '--dll', 'msvcp140.dll', '--xvd', $trXvd, '--scratch', $scratch, '--out', (Join-Path $Evidence 'terraria_msvcp140_load.json'))
Invoke-Probe @('load', '--title', 'Terraria', '--bin-dir', $trMount, '--dll', 'Data/Native/msvcp140.dll', '--xvd', $trXvd, '--scratch', $scratch, '--out', (Join-Path $Evidence 'terraria_native_msvcp140_load.json'))
Invoke-Probe @('load', '--title', 'Terraria', '--bin-dir', $trMount, '--dll', 'Data/Plugins/PartyXboxLive.dll', '--search', 'Data/Native', '--xvd', $trXvd, '--scratch', $scratch, '--pinvoke-from', (Join-Path $trMount 'Data\Native\GameAssembly.dll'), '--pinvoke-from', (Join-Path $trMount 'Data\Managed\Metadata\global-metadata.dat'), '--pinvoke-prefix', 'PartyXbl', '--out', (Join-Path $Evidence 'terraria_partyxboxlive_load.json'))
Invoke-Probe @('token', '--token', 'xboxlive.dll', '--root', $trMount, '--root', (Join-Path $trXvd 'Windows\System32'), '--out', (Join-Path $Evidence 'terraria_xboxlive_token.json'))
Invoke-Probe @('scan', '--title', 'Fortnite', '--bin-dir', $fnBin, '--xvd', $fnXvd, '--stage-dir', $dist, '--out', (Join-Path $Evidence 'fortnite_scan.json'))
Invoke-Probe @('scan', '--title', 'Trackmania', '--bin-dir', $tmBin, '--xvd', $tmXvd, '--stage-dir', $dist, '--out', (Join-Path $Evidence 'trackmania_scan.json'))
Invoke-Probe @('scan', '--title', 'Terraria', '--bin-dir', $trMount, '--xvd', $trXvd, '--stage-dir', $dist, '--out', (Join-Path $Evidence 'terraria_scan.json'))
