param(
  [Parameter(Mandatory = $true)][ValidateSet('Terraria', 'Trackmania')][string]$Title,
  [string]$Games = 'D:\xbox-pc-port\games',
  [string]$OverlayRoot = 'D:\xbox-pc-port\emulator\run-xone',
  [string]$Target = 'D:\xbox-pc-port\emulator\target\release',
  [string]$XoneTarget = 'D:\xbox-pc-port\emulator\target-xone\release',
  [string]$D3d12Xs = '',
  [string]$XAudio2Dll = '',
  [string]$Dxc64 = 'D:\xbox-pc-port\emulator\third_party\dxc\v1.9.2609\bin\x64',
  [string]$Drive = 'X:',
  [int]$Seconds = 0,
  [string]$Agility = '',
  [int]$AgilityVersion = 620,
  [switch]$D3d12Debug,
  [string]$GameArgs = '',
  [switch]$StageOnly,
  [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$Exe = @{ Terraria = 'Terraria.exe'; Trackmania = 'GbxGame.exe' }[$Title]
$LogDir = Join-Path $env:TEMP "tombox-$($Title.ToLower())"
$Log    = Join-Path $LogDir 'tombox_compat.log'
$RunTxt = Join-Path $LogDir 'run.txt'
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

function L($m){ Write-Host "  $m" -ForegroundColor DarkGray }

function Mirror-Dir([string]$src, [string]$dst, [string[]]$skip) {
  New-Item -ItemType Directory -Force -Path $dst | Out-Null
  foreach ($c in Get-ChildItem $src -Force) {
    if ($skip -contains $c.Name.ToLower()) { continue }
    $to = Join-Path $dst $c.Name
    if (Test-Path $to) { continue }
    if ($c.PSIsContainer) {
      New-Item -ItemType Junction -Path $to -Target $c.FullName | Out-Null
    } else {
      New-Item -ItemType HardLink -Path $to -Target $c.FullName | Out-Null
    }
  }
}

function Stage-File([string]$from, [string]$dir, [string]$name) {
  if (-not (Test-Path $from)) { throw "missing runtime DLL $from (build it first)" }
  $to = Join-Path $dir $name
  if (Test-Path $to) { Remove-Item $to -Force }
  Copy-Item $from $to
}

$Pkg = Get-ChildItem (Join-Path $Games "$Title\extracted") -Directory |
       Where-Object { Test-Path (Join-Path $_.FullName "Mount\$Exe") } | Select-Object -First 1
if (-not $Pkg) { throw "no extracted package with Mount\$Exe under $Games\$Title\extracted" }
$Mount  = Join-Path $Pkg.FullName 'Mount'
$OMount = Join-Path $OverlayRoot "$Title\Mount"

$xone = @('d3d12_x', 'xg_x', 'dxcompiler_x', 'xfrontpaneldisplay')
$series = @('d3d12_xs', 'acphal', 'xmem', 'mfplat')
$dxc = @('dxcompiler.dll', 'dxil.dll')
$staged = @($xone + $series | ForEach-Object { "$_.dll" }) + @('mfplat_sys.dll', 'xaudio2_9.dll') + $dxc
. (Join-Path $PSScriptRoot 'stage_compat.ps1')
$Xvd = Join-Path $Pkg.FullName 'EmbeddedXvd'
$Protected = @($Games, $Pkg.FullName)
Assert-OutsideDump $OMount $Protected
if ($DryRun) {
  $plan = Invoke-CompatStaging -Dest $OMount -Targets @($Target, $XoneTarget) -Xvd $Xvd -Mount $Mount -Explicit $staged -Protected $Protected -DryRun
  Write-CompatStaging $plan $OMount -DryRun
  L "script-staged: $($staged -join ', ')"
  L 'dry run; nothing written, not launching'
  return
}
Mirror-Dir $Mount $OMount $staged
L "overlay $OMount (dump untouched)"
$plan = Invoke-CompatStaging -Dest $OMount -Targets @($Target, $XoneTarget) -Xvd $Xvd -Mount $Mount -Explicit $staged -Protected $Protected
Write-CompatStaging $plan $OMount

foreach ($d in $xone)   { Stage-File (Join-Path $XoneTarget "$d.dll") $OMount "$d.dll" }
foreach ($d in $series) { Stage-File (Join-Path $Target "$d.dll") $OMount "$d.dll" }
if ($D3d12Xs) { Stage-File $D3d12Xs $OMount 'd3d12_xs.dll'; L "d3d12_xs override $D3d12Xs" }
Stage-File "$env:WINDIR\System32\mfplat.dll" $OMount 'mfplat_sys.dll'
foreach ($d in $dxc) { Stage-File (Join-Path $Dxc64 $d) $OMount $d }
$XA2 = $XAudio2Dll
if (-not $XA2) {
  $XA2 = Join-Path $Target 'XAudio2_9.dll'
  if (-not (Test-Path $XA2)) { $XA2 = 'D:\xbox-pc-port\emulator\target-xaudio\release\XAudio2_9.dll' }
}
if (Test-Path $XA2) {
  Stage-File $XA2 $OMount 'XAudio2_9.dll'
  L "staged XAudio2_9.dll compat from $XA2 (app dir wins over System32; not a KnownDLL)"
} else {
  $XA2Dst = Join-Path $OMount 'XAudio2_9.dll'
  if (Test-Path $XA2Dst) { Remove-Item $XA2Dst -Force }
  L "XAudio2_9.dll compat not found ($XA2); $Title loads System32 XAudio2_9.dll (build: cargo build --release -p xaudio2_9)"
}
L "staged DXC from $Dxc64 (dxcompiler_x loads the dxcompiler.dll next to it)"
L "staged $($xone -join ', ') from $XoneTarget"
L "staged $($series -join ', ') from $Target (d3d12_x forwards device creation to the d3d12_xs next to it)"

if ($StageOnly) { L 'stage only; not launching'; return }

$current = (subst) | Where-Object { $_ -match "^$([regex]::Escape($Drive))\\" }
if ($current -and $current -notmatch [regex]::Escape($OMount)) { throw "$Drive is already substituted ($current); pass -Drive" }
if (-not $current) { subst $Drive $OMount }
L "$Drive -> $OMount"

Remove-Item Env:TOMBOX_NET_LOCAL_PORT -EA 0
if ($Agility) {
  if (-not (Test-Path (Join-Path $Agility 'D3D12Core.dll'))) { throw "no D3D12Core.dll in $Agility" }
  $env:TOMBOX_AGILITY_PATH = $Agility
  $env:TOMBOX_AGILITY_VERSION = "$AgilityVersion"
  L "Agility SDK $AgilityVersion from $Agility (SetSDKVersion, no system files touched)"
} else { Remove-Item Env:TOMBOX_AGILITY_PATH, Env:TOMBOX_AGILITY_VERSION -EA 0 }
if ($D3d12Debug) { $env:TOMBOX_D3D12_DEBUG = '1' } else { Remove-Item Env:TOMBOX_D3D12_DEBUG -EA 0 }
$env:TOMBOX_NET_REDIRECT = '127.0.0.1'
$env:TOMBOX_NET_BACKEND = 'builtin'
Remove-Item $Log, $RunTxt -Force -EA 0
$env:TOMBOX_LOG = $Log
$env:_NO_DEBUG_HEAP = '1'
$env:MDBG_LIMIT = '86400'
$exePath = "$Drive\$Exe"
$mdbg = Join-Path $Target 'mdbg.exe'
$sw = [Diagnostics.Stopwatch]::StartNew()
$argList = @("`"$exePath`"")
if ($GameArgs) { $argList += $GameArgs }
$p = Start-Process -FilePath $mdbg -ArgumentList $argList -WorkingDirectory "$Drive\" `
       -RedirectStandardOutput $RunTxt -RedirectStandardError (Join-Path $LogDir 'run.err.txt') `
       -WindowStyle Hidden -PassThru
L "launched $Title under mdbg pid=$($p.Id); log $Log"
if ($Seconds -gt 0) {
  if (-not $p.WaitForExit($Seconds * 1000)) { L 'time limit'; try { $p.Kill() } catch {} }
} else {
  $p.WaitForExit()
}
L ("exited after {0:N1}s" -f $sw.Elapsed.TotalSeconds)
