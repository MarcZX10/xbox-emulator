param(
  [string]$Dump    = 'D:\xbox-pc-port\games\XDefiant\extracted',
  [string]$Overlay = 'D:\xbox-pc-port\emulator\run-xdefiant',
  [string]$Target  = 'D:\xbox-pc-port\emulator\target\release',
  [string]$D3d12Xs = '',
  [string]$XAudio2Dll = '',
  [string]$Drive   = 'Z:',
  [int]$Seconds = 0,
  [string]$GameArgs = '',
  [switch]$StageOnly,
  [switch]$FileTrace,
  [string]$Mdbg = '',
  [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$Exe    = 'Rogue_scarlett_rtx.exe'
$LogDir = Join-Path $env:TEMP 'tombox-xdefiant'
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
  if (-not (Test-Path $from)) { throw "missing runtime file $from (build it first)" }
  $to = Join-Path $dir $name
  if (Test-Path $to) { Remove-Item $to -Force }
  Copy-Item $from $to
}

$Mount  = Join-Path $Dump 'Mount'
$OMount = Join-Path $Overlay 'Mount'
if (-not (Test-Path (Join-Path $Mount $Exe))) { throw "no $Exe under $Mount" }

$series = @('acphal', 'AudioHALWrapper', 'chad', 'd3d12_xs', 'dxcompiler_xs', 'mfplat', 'xapu', 'xcrdapi', 'xcurl', 'xmem')
$staged = @($series | ForEach-Object { "$_.dll".ToLower() }) + @('mfplat_sys.dll', 'xg_xs.dll', 'xaudio2_9.dll')
. (Join-Path $PSScriptRoot 'stage_compat.ps1')
$Xvd = Join-Path $Dump 'EmbeddedXvd'
$Protected = @('D:\xbox-pc-port\games', $Dump)
Assert-OutsideDump $OMount $Protected
if ($DryRun) {
  $plan = Invoke-CompatStaging -Dest $OMount -Targets @($Target) -Xvd $Xvd -Mount $Mount -Explicit $staged -Protected $Protected -DryRun
  Write-CompatStaging $plan $OMount -DryRun
  L "script-staged: $($staged -join ', ')"
  L 'dry run; nothing written, not launching'
  return
}
Mirror-Dir $Mount $OMount $staged
L "overlay $OMount (dump untouched)"
$plan = Invoke-CompatStaging -Dest $OMount -Targets @($Target) -Xvd $Xvd -Mount $Mount -Explicit $staged -Protected $Protected
Write-CompatStaging $plan $OMount

foreach ($d in $series) { Stage-File (Join-Path $Target "$d.dll") $OMount "$d.dll" }
if ($D3d12Xs) { Stage-File $D3d12Xs $OMount 'd3d12_xs.dll'; L "d3d12_xs override $D3d12Xs" }
Stage-File (Join-Path $Dump 'EmbeddedXvd\Windows\System32\xg_xs.dll') $OMount 'xg_xs.dll'
Stage-File "$env:WINDIR\System32\mfplat.dll" $OMount 'mfplat_sys.dll'
L "staged $($series -join ', ') from $Target, console xg_xs from the dump"
$XA2 = $XAudio2Dll
if (-not $XA2) {
  $XA2 = Join-Path $Target 'XAudio2_9.dll'
  if (-not (Test-Path $XA2)) { $XA2 = 'D:\xbox-pc-port\emulator\target-xaudio\release\XAudio2_9.dll' }
}
if (Test-Path $XA2) {
  Stage-File $XA2 $OMount 'XAudio2_9.dll'
  L "staged XAudio2_9.dll compat from $XA2 (bink2scarlett imports XAudio2_9 #1; app dir wins over System32)"
} else {
  $XA2Dst = Join-Path $OMount 'XAudio2_9.dll'
  if (Test-Path $XA2Dst) { Remove-Item $XA2Dst -Force }
  L "XAudio2_9.dll compat not found ($XA2); bink2scarlett binds System32 XAudio2_9.dll (build: cargo build --release -p xaudio2_9)"
}

if ($StageOnly) { L 'stage only; not launching'; return }

$current = (subst) | Where-Object { $_ -match "^$([regex]::Escape($Drive))\\" }
if ($current -and $current -notmatch [regex]::Escape($OMount)) { throw "$Drive is already substituted ($current); pass -Drive" }
if (-not $current) { subst $Drive $OMount }
L "$Drive -> $OMount"

Remove-Item Env:TOMBOX_NET_LOCAL_PORT -EA 0
$env:TOMBOX_NET_REDIRECT = '127.0.0.1'
$env:TOMBOX_NET_BACKEND = 'builtin'
$env:TOMBOX_TEXTURE_VA = '1'
if ($FileTrace) { $env:TOMBOX_GDK_FILE_TRACE = '1'; $env:TOMBOX_GDK_LOG_LIMIT = '200000'; L 'gdk file trace on' } else { Remove-Item Env:TOMBOX_GDK_FILE_TRACE -EA 0 }
L "online services: emulated in-process (builtin); TOMBOX_XDEFIANT_NOTIFICATIONS defaults on"

Remove-Item $Log, $RunTxt -Force -EA 0
$env:TOMBOX_LOG = $Log
$env:_NO_DEBUG_HEAP = '1'
$env:MDBG_LIMIT = '86400'
$exePath = "$Drive\$Exe"
$mdbg = if ($Mdbg) { $Mdbg } else { Join-Path $Target 'mdbg.exe' }
$sw = [Diagnostics.Stopwatch]::StartNew()
$argList = @("`"$exePath`"")
if ($GameArgs) { $argList += $GameArgs }
$p = Start-Process -FilePath $mdbg -ArgumentList $argList -WorkingDirectory "$Drive\" `
       -RedirectStandardOutput $RunTxt -RedirectStandardError (Join-Path $LogDir 'run.err.txt') `
       -WindowStyle Hidden -PassThru
L "launched XDefiant under mdbg pid=$($p.Id); log $Log"
if ($Seconds -gt 0) {
  if (-not $p.WaitForExit($Seconds * 1000)) { L 'time limit'; try { $p.Kill() } catch {} }
} else {
  $p.WaitForExit()
}
L ("exited after {0:N1}s" -f $sw.Elapsed.TotalSeconds)
