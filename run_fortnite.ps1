param(
  [string]$Dump    = 'D:\xbox-pc-port\games\Fortnite\extracted',
  [string]$Overlay = 'D:\xbox-pc-port\emulator\run',
  [string]$DStorage = 'D:\xbox-pc-port\emulator\runtime\dstorage-1.3.0',
  [int]$Seconds = 0,
  [string]$GameArgs = '',
  [string]$Target = 'D:\xbox-pc-port\emulator\target\release',
  [string]$XAudio2Dll = '',
  [ValidateSet('Off','Replay','Map','Play')][string]$Match = 'Off',
  [string]$MatchMap = '/Game/Athena/Apollo/Maps/Apollo_Terrain',
  [string]$GameMode = '/Game/Athena/Athena_GameMode.Athena_GameMode_C',
  [string]$Playlist = 'Playlist_DefaultSquad',
  [string]$Replay = 'BR_FortBench',
  [string]$GameServer = '127.0.0.1:7777',
  [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$exe  = 'G:\FortniteGame\Binaries\XSX\FortniteClient-XSX-Shipping.exe'
foreach ($v in 'TOMBOX_FN_MATCH','TOMBOX_FN_GAMESERVER','TOMBOX_FN_PLAYLIST','TOMBOX_FN_GAMEMODE','TOMBOX_FN_MAP') { Remove-Item "Env:$v" -EA 0 }
switch ($Match) {
  'Replay' {
    $env:TOMBOX_FN_MATCH = 'replay'
    $argList = @("`"$exe`"", "-REPLAY=$Replay", '-unattended', '-REPLAYSTREAMER=LocalFileNetworkReplayStreaming', '-ReplayStreamerDemoPath=../../../FortniteGame/Content/DefaultReplays/')
  }
  'Map' {
    $env:TOMBOX_FN_MATCH = 'map'
    $env:TOMBOX_FN_MAP = $MatchMap
    $env:TOMBOX_FN_GAMEMODE = $GameMode
    $env:TOMBOX_FN_PLAYLIST = $Playlist
    $argList = @("`"$exe`"", "${MatchMap}?game=$GameMode", '-unattended')
  }
  'Play' {
    $env:TOMBOX_FN_MATCH = 'play'
    $env:TOMBOX_FN_GAMESERVER = $GameServer
    $env:TOMBOX_FN_PLAYLIST = $Playlist
    $env:TOMBOX_FN_MAP = $MatchMap
    $env:TOMBOX_FN_GAMEMODE = $GameMode
    $argList = @("`"$exe`"", '-unattended')
  }
  default {
    $argList = @("`"$exe`"", '-unattended')
  }
}
if ($GameArgs) { $argList += $GameArgs }
. (Join-Path $PSScriptRoot 'stage_compat.ps1')
$Shared = 'D:\xbox-pc-port\emulator\target\release'
$Xvd = Join-Path $Dump 'EmbeddedXvd'
$OBinPlan = Join-Path $Overlay 'Mount\FortniteGame\Binaries\XSX'
$ScriptStaged = @('xg_xs.dll', 'acphal.dll', 'd3d12_xs.dll', 'dxcompiler_xs.dll', 'mfplat.dll', 'xcrdapi.dll', 'xmem.dll',
                  'AudioHALWrapper.dll', 'opus.dll', 'SKP_SILK_SDK.dll', 'XAudio2_9.dll', 'dstorage.dll', 'dstoragecore.dll', 'mfplat_sys.dll')
$Protected = @('D:\xbox-pc-port\games', $Dump)
if ($DryRun) {
  $plan = Invoke-CompatStaging -Dest $OBinPlan -Targets @($Target, $Shared) -Xvd $Xvd -Mount (Join-Path $Dump 'Mount\FortniteGame\Binaries\XSX') -Explicit $ScriptStaged -Protected $Protected -DryRun
  Write-CompatStaging $plan $OBinPlan -DryRun
  Write-Host "match mode: $Match"
  Write-Host "target: $Target"
  foreach ($v in 'TOMBOX_FN_MATCH','TOMBOX_FN_GAMESERVER','TOMBOX_FN_PLAYLIST','TOMBOX_FN_GAMEMODE','TOMBOX_FN_MAP') {
    $val = [Environment]::GetEnvironmentVariable($v)
    Write-Host ("{0}={1}" -f $v, $(if ($null -eq $val) { '<unset>' } else { $val }))
  }
  Write-Host ("argList: " + ($argList -join ' '))
  foreach ($v in 'TOMBOX_FN_MATCH','TOMBOX_FN_GAMESERVER','TOMBOX_FN_PLAYLIST','TOMBOX_FN_GAMEMODE','TOMBOX_FN_MAP') { Remove-Item "Env:$v" -EA 0 }
  return
}
$LogDir = Join-Path $env:TEMP 'tombox'
$Log    = Join-Path $LogDir 'tombox_compat.log'
$RunTxt = Join-Path $LogDir 'run.txt'
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

function L($m){ Write-Host "  $m" -ForegroundColor DarkGray }

function Mirror-Dir([string]$src, [string]$dst, [string[]]$keepReal) {
  New-Item -ItemType Directory -Force -Path $dst | Out-Null
  foreach ($c in Get-ChildItem $src -Force) {
    $to = Join-Path $dst $c.Name
    if (Test-Path $to) { continue }
    if ($c.PSIsContainer) {
      if ($keepReal -contains $c.Name) { continue }
      New-Item -ItemType Junction -Path $to -Target $c.FullName | Out-Null
    } else {
      New-Item -ItemType HardLink -Path $to -Target $c.FullName | Out-Null
    }
  }
}

$Mount = Join-Path $Dump 'Mount'
$OMount = Join-Path $Overlay 'Mount'
Mirror-Dir $Mount $OMount @('FortniteGame')
Mirror-Dir (Join-Path $Mount 'FortniteGame') (Join-Path $OMount 'FortniteGame') @('Binaries')
Mirror-Dir (Join-Path $Mount 'FortniteGame\Binaries') (Join-Path $OMount 'FortniteGame\Binaries') @('XSX')
$OBin = Join-Path $OMount 'FortniteGame\Binaries\XSX'
Mirror-Dir (Join-Path $Mount 'FortniteGame\Binaries\XSX') $OBin @()
L "overlay $OMount (dump untouched)"

$plan = Invoke-CompatStaging -Dest $OBin -Targets @($Target, $Shared) -Xvd $Xvd -Mount (Join-Path $Mount 'FortniteGame\Binaries\XSX') -Explicit $ScriptStaged -Protected $Protected
Write-CompatStaging $plan $OBin
Copy-IntoOverlay (Join-Path $Dump 'EmbeddedXvd\Windows\System32\xg_xs.dll') $OBin 'xg_xs.dll' $Protected
foreach ($d in 'acphal','d3d12_xs','dxcompiler_xs','mfplat','xcrdapi','xmem') {
  Copy-IntoOverlay (Join-Path $Target "$d.dll") $OBin "$d.dll" $Protected
}
function From-Target([string]$n) { $t = Join-Path $Target $n; if (Test-Path $t) { $t } else { Join-Path $Shared $n } }
$AHW = From-Target 'AudioHALWrapper.dll'
if (Test-Path $AHW) {
  Copy-IntoOverlay $AHW $OBin 'AudioHALWrapper.dll' $Protected
  L "AudioHALWrapper.dll compat staged (GameChat2 loads it by name; the console OS DLL has no PC copy)"
} else {
  L "AudioHALWrapper.dll compat not found ($AHW); GameChat2 voice codec unavailable (build: cargo build --release -p audiohalwrapper)"
}
foreach ($c in 'opus','SKP_SILK_SDK') {
  $cd = From-Target "$c.dll"
  if (Test-Path $cd) {
    Copy-IntoOverlay $cd $OBin "$c.dll" $Protected
    L "$c.dll codec staged for AudioHALWrapper (libopus third_party/opus 316c675fa1c9)"
  } else {
    L "$c.dll codec not found ($cd); AudioHALWrapper reports E_MOD_NOT_FOUND for that codec (build: cargo build --release -p opus_dll -p skp_silk)"
  }
}
$XA2 = $XAudio2Dll
if (-not $XA2) {
  $XA2 = From-Target 'XAudio2_9.dll'
  if (-not (Test-Path $XA2)) { $XA2 = 'D:\xbox-pc-port\emulator\target-xaudio\release\XAudio2_9.dll' }
}
$XA2Dst = Join-Path $OBin 'XAudio2_9.dll'
if (Test-Path $XA2Dst) { Remove-Item $XA2Dst -Force }
if (Test-Path $XA2) {
  Copy-IntoOverlay $XA2 $OBin 'XAudio2_9.dll' $Protected
  L "XAudio2_9.dll compat staged from $XA2 (app dir wins over System32; not a KnownDLL)"
} else {
  L "XAudio2_9.dll compat not found ($XA2); game loads System32 XAudio2_9.dll (build: cargo build --release -p xaudio2_9)"
}
foreach ($d in 'dstorage.dll','dstoragecore.dll') { Copy-IntoOverlay (Join-Path $DStorage $d) $OBin $d $Protected }
Copy-IntoOverlay "$env:WINDIR\System32\mfplat.dll" $OBin 'mfplat_sys.dll' $Protected
L "staged TomBox runtime DLLs into overlay"

if ((subst) -match '^G:\\') { subst G: /d | Out-Null }
subst G: $OMount
L "G: -> $OMount"

Remove-Item Env:TOMBOX_NET_LOCAL_PORT -EA 0
$env:TOMBOX_NET_REDIRECT = '127.0.0.1'
$env:TOMBOX_NET_BACKEND = 'builtin'
L "online services: emulated in-process (builtin)"

Remove-Item $Log,$RunTxt -Force -EA 0
$env:TOMBOX_LOG = $Log
$env:_NO_DEBUG_HEAP = '1'
$env:MDBG_LIMIT = '86400'
$mdbg = 'D:\xbox-pc-port\emulator\target\release\mdbg.exe'
$sw = [Diagnostics.Stopwatch]::StartNew()
if ($Match -ne 'Off') { L ("match mode {0}: {1}" -f $Match, ($argList -join ' ')) }
$p = Start-Process -FilePath $mdbg -ArgumentList $argList `
       -RedirectStandardOutput $RunTxt -RedirectStandardError (Join-Path $LogDir 'run.err.txt') `
       -WindowStyle Hidden -PassThru
L "launched mdbg pid=$($p.Id)"
if ($Seconds -gt 0) {
  if (-not $p.WaitForExit($Seconds * 1000)) { L "time limit"; try { $p.Kill() } catch {} }
} else {
  $p.WaitForExit()
}
L ("exited after {0:N1}s" -f $sw.Elapsed.TotalSeconds)
foreach ($v in 'TOMBOX_FN_MATCH','TOMBOX_FN_GAMESERVER','TOMBOX_FN_PLAYLIST','TOMBOX_FN_GAMEMODE','TOMBOX_FN_MAP') { Remove-Item "Env:$v" -EA 0 }
