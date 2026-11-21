function Get-ConsoleOsBuild([string]$Xvd) {
  $nt = Join-Path $Xvd 'Windows\System32\ntdll.dll'
  if (-not (Test-Path $nt)) { return 0 }
  $v = (Get-Item $nt).VersionInfo
  if ($v.FileBuildPart) { return [int]$v.FileBuildPart }
  return [int](($v.FileVersion -split '[. ]')[2])
}

function Get-NormalDllKey([string]$n) {
  return ([IO.Path]::GetFileNameWithoutExtension($n).ToLower() -replace '[^a-z0-9]', '_')
}

function Get-PeSysForwarders([string]$Path) {
  $out = @()
  if (-not (Test-Path -LiteralPath $Path)) { return $out }
  $b = [IO.File]::ReadAllBytes($Path)
  if ($b.Length -lt 0x40 -or $b[0] -ne 0x4D -or $b[1] -ne 0x5A) { return $out }
  $nt = [BitConverter]::ToInt32($b, 0x3C)
  if ($nt -lt 0 -or $nt + 24 -gt $b.Length -or [BitConverter]::ToUInt32($b, $nt) -ne 0x4550) { return $out }
  $nsec = [BitConverter]::ToUInt16($b, $nt + 6)
  $optSize = [BitConverter]::ToUInt16($b, $nt + 20)
  $opt = $nt + 24
  $dirOff = if ([BitConverter]::ToUInt16($b, $opt) -eq 0x20B) { $opt + 112 } else { $opt + 96 }
  $erva = [BitConverter]::ToUInt32($b, $dirOff)
  $esize = [BitConverter]::ToUInt32($b, $dirOff + 4)
  if ($erva -eq 0) { return $out }
  $secs = @(for ($i = 0; $i -lt $nsec; $i++) {
    $s = $opt + $optSize + $i * 40
    [pscustomobject]@{ VSize = [BitConverter]::ToUInt32($b, $s + 8); Va = [BitConverter]::ToUInt32($b, $s + 12); RSize = [BitConverter]::ToUInt32($b, $s + 16); Raw = [BitConverter]::ToUInt32($b, $s + 20) }
  })
  $off = { param([uint32]$r) foreach ($s in $secs) { if ($r -ge $s.Va -and $r -lt $s.Va + [Math]::Max($s.VSize, $s.RSize)) { return [int]($r - $s.Va + $s.Raw) } }; return -1 }
  $e = & $off $erva
  if ($e -lt 0) { return $out }
  $nfun = [BitConverter]::ToUInt32($b, $e + 20)
  $fun = & $off ([BitConverter]::ToUInt32($b, $e + 28))
  if ($fun -lt 0) { return $out }
  $seen = @{}
  for ($i = 0; $i -lt [Math]::Min($nfun, 65536); $i++) {
    $f = [BitConverter]::ToUInt32($b, $fun + 4 * $i)
    if ($f -lt $erva -or $f -ge $erva + $esize) { continue }
    $o = & $off $f
    if ($o -lt 0) { continue }
    $end = [Array]::IndexOf($b, [byte]0, $o)
    if ($end -lt 0) { continue }
    $target = [Text.Encoding]::ASCII.GetString($b, $o, $end - $o)
    $dot = $target.LastIndexOf('.')
    if ($dot -lt 5) { continue }
    $dll = $target.Substring(0, $dot)
    if ($dll -match '^(.+)_sys$' -and -not $seen.ContainsKey($dll.ToLower())) {
      $seen[$dll.ToLower()] = $true
      $out += [pscustomobject]@{ Name = "$dll.dll"; From = (Join-Path "$env:WINDIR\System32" "$($Matches[1]).dll"); Kind = 'sys' }
    }
  }
  return $out
}

function Get-CompatDllPlan([string]$Root, [string]$Xvd) {
  $ws = Get-Content (Join-Path $Root 'Cargo.toml') -Raw
  $members = [regex]::Matches($ws, '"(crates/compat/[^"]+)"') | ForEach-Object { $_.Groups[1].Value }
  $sys32 = if ($Xvd) { Join-Path $Xvd 'Windows\System32' } else { '' }
  $consoleNames = @{}
  if ($sys32 -and (Test-Path $sys32)) {
    foreach ($f in Get-ChildItem $sys32 -Filter *.dll -File) { $consoleNames[(Get-NormalDllKey $f.Name)] = $f.Name }
  }
  $plan = @()
  foreach ($m in $members) {
    $dir = Join-Path $Root ($m -replace '/', '\')
    $toml = Join-Path $dir 'Cargo.toml'
    if (-not (Test-Path $toml)) { $plan += [pscustomobject]@{ Crate = $m; Lib = ''; Dll = ''; Built = ''; Live = 0; Companions = @(); Skip = 'no Cargo.toml' }; continue }
    $t = Get-Content $toml -Raw
    $ct = [regex]::Match($t, 'crate-type\s*=\s*\[([^\]]*)\]')
    if (-not $ct.Success -or $ct.Groups[1].Value -notmatch 'cdylib') { continue }
    $pkg = [regex]::Match($t, '\[package\][^\[]*?name\s*=\s*"([^"]+)"').Groups[1].Value
    $lm = [regex]::Match($t, '\[lib\][^\[]*?name\s*=\s*"([^"]+)"')
    $lib = if ($lm.Success) { $lm.Groups[1].Value } else { $pkg -replace '-', '_' }
    $key = Get-NormalDllKey $lib
    $dll = if ($consoleNames.ContainsKey($key)) { $consoleNames[$key] } else { "$lib.dll" }
    $live = -1
    $ex = Join-Path $dir 'exports.txt'
    if (Test-Path $ex) {
      $live = @(Get-Content $ex | Where-Object { $f = ($_ -split ' \| ')[0].Trim() -split '\s+'; $f.Count -ge 3 -and $f[2] -ne 'missing' }).Count
    }
    $comp = @()
    $src = Join-Path $dir 'src'
    if (Test-Path $src) {
      $lits = Get-ChildItem $src -Recurse -Filter *.rs | Select-String -Pattern '"([A-Za-z0-9_.]+)_(sys|console)\.dll"' -AllMatches |
              ForEach-Object { $_.Matches } | ForEach-Object { "$($_.Groups[1].Value)|$($_.Groups[2].Value)" } | Sort-Object -Unique
      foreach ($l in $lits) {
        $base, $kind = $l -split '\|'
        $name = "${base}_$kind.dll"
        if ($kind -eq 'sys') {
          $from = Join-Path "$env:WINDIR\System32" "$base.dll"
        } else {
          $bk = Get-NormalDllKey $base
          $cn = if ($consoleNames.ContainsKey($bk)) { $consoleNames[$bk] } else { "$base.dll" }
          $from = if ($sys32) { Join-Path $sys32 $cn } else { '' }
        }
        $comp += [pscustomobject]@{ Name = $name; From = $from; Kind = $kind }
      }
    }
    $plan += [pscustomobject]@{ Crate = $m; Lib = $lib; Dll = $dll; Built = "$lib.dll"; Live = $live; Companions = $comp; Skip = '' }
  }
  return $plan
}

function Select-CompatDllPlan($Plan, [int]$OsBuild) {
  $libs = @{}
  foreach ($p in $Plan) { if ($p.Lib) { $libs[$p.Lib.ToLower()] = $p } }
  foreach ($p in $Plan) {
    if ($p.Skip -or -not $p.Lib) { continue }
    if ($p.Live -eq 0) { $p.Skip = 'exports.txt has no implemented/forwarded export'; continue }
    $vm = [regex]::Match($p.Lib, '^(.+)_(\d{5})$')
    if ($vm.Success -and $libs.ContainsKey($vm.Groups[1].Value.ToLower())) {
      $vb = [int]$vm.Groups[2].Value
      if ($OsBuild -and (($OsBuild -lt 22000) -ne ($vb -lt 22000))) { $p.Skip = "variant for OS $vb, title OS $OsBuild" }
      continue
    }
    if ($OsBuild) {
      $variants = @($Plan | Where-Object { $_.Lib -match ('^' + [regex]::Escape($p.Lib) + '_(\d{5})$') })
      foreach ($v in $variants) {
        $vb = [int]([regex]::Match($v.Lib, '_(\d{5})$').Groups[1].Value)
        if (($OsBuild -lt 22000) -eq ($vb -lt 22000)) { $p.Skip = "title OS $OsBuild uses $($v.Lib).dll"; break }
      }
    }
  }
  return $Plan
}

function Resolve-RealPath([string]$Path) {
  $full = [IO.Path]::GetFullPath($Path).TrimEnd('\')
  for ($guard = 0; $guard -lt 32; $guard++) {
    $parts = $full.Split('\')
    $changed = $false
    for ($i = 1; $i -lt $parts.Count; $i++) {
      $prefix = ($parts[0..$i] -join '\')
      if (-not (Test-Path -LiteralPath $prefix)) { break }
      $it = Get-Item -LiteralPath $prefix -Force
      if ($it.LinkType -in @('Junction', 'SymbolicLink') -and $it.Target) {
        $t = @($it.Target)[0]
        if (-not [IO.Path]::IsPathRooted($t)) { $t = Join-Path (Split-Path $prefix -Parent) $t }
        $rest = if ($i + 1 -lt $parts.Count) { $parts[($i + 1)..($parts.Count - 1)] -join '\' } else { '' }
        $full = [IO.Path]::GetFullPath($(if ($rest) { Join-Path $t $rest } else { $t })).TrimEnd('\')
        $changed = $true
        break
      }
    }
    if (-not $changed) { return $full }
  }
  throw "too many reparse points resolving $Path"
}

function Assert-OutsideDump([string]$Path, [string[]]$Protected) {
  $full = (Resolve-RealPath $Path) + '\'
  foreach ($d in $Protected) {
    if (-not $d) { continue }
    $pd = (Resolve-RealPath $d) + '\'
    if ($full.StartsWith($pd, [StringComparison]::OrdinalIgnoreCase)) { throw "refusing to stage into the game dump: $Path is under $d" }
  }
}

function Copy-IntoOverlay([string]$From, [string]$Dest, [string]$Name, [string[]]$Protected) {
  Assert-OutsideDump $Dest $Protected
  $to = Join-Path $Dest $Name
  if (Test-Path $to) { Remove-Item $to -Force }
  Copy-Item $From $to
}

function Invoke-CompatStaging {
  param(
    [string]$Root = 'D:\xbox-pc-port\emulator',
    [Parameter(Mandatory = $true)][string]$Dest,
    [string[]]$Targets = @('D:\xbox-pc-port\emulator\target\release'),
    [string]$Xvd = '',
    [string]$Mount = '',
    [string[]]$Explicit = @(),
    [string[]]$Protected = @('D:\xbox-pc-port\games'),
    [switch]$DryRun
  )
  Assert-OutsideDump $Dest $Protected
  $os = if ($Xvd) { Get-ConsoleOsBuild $Xvd } else { 0 }
  $plan = Select-CompatDllPlan (Get-CompatDllPlan $Root $Xvd) $os
  $explicitSet = @{}
  foreach ($e in $Explicit) { $explicitSet[$e.ToLower()] = $true }
  $shipped = @{}
  if ($Mount -and (Test-Path $Mount)) { foreach ($f in Get-ChildItem $Mount -File -Filter *.dll) { $shipped[$f.Name.ToLower()] = $true } }
  $rows = @()
  foreach ($p in $plan) {
    if (-not $p.Lib) { $rows += [pscustomobject]@{ Dll = '-'; Action = 'skip'; Source = $p.Crate; Why = $p.Skip }; continue }
    if ($p.Skip) { $rows += [pscustomobject]@{ Dll = $p.Dll; Action = 'skip'; Source = $p.Crate; Why = $p.Skip }; continue }
    if ($explicitSet.ContainsKey($p.Dll.ToLower())) { $rows += [pscustomobject]@{ Dll = $p.Dll; Action = 'script'; Source = $p.Crate; Why = 'staged by this run script' }; continue }
    if ($shipped.ContainsKey($p.Dll.ToLower())) { $rows += [pscustomobject]@{ Dll = $p.Dll; Action = 'skip'; Source = $p.Crate; Why = 'title package ships its own copy' }; continue }
    $own = @($p.Companions | Where-Object { $_.Kind -eq 'console' -and $_.From -and (Get-NormalDllKey ($_.Name -replace '_console\.dll$', '')) -eq (Get-NormalDllKey $p.Lib) }) | Select-Object -First 1
    if ($own -and (Test-Path (Split-Path $own.From -Parent)) -and -not (Test-Path $own.From)) { $rows += [pscustomobject]@{ Dll = $p.Dll; Action = 'skip'; Source = $p.Crate; Why = "forwarder not staged: title dump has no $(Split-Path $own.From -Leaf) for $($own.Name)" }; continue }
    $from = $Targets | ForEach-Object { Join-Path $_ $p.Built } | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $from) { $rows += [pscustomobject]@{ Dll = $p.Dll; Action = 'missing'; Source = $p.Built; Why = "not built (cargo build --release -p $(Split-Path $p.Crate -Leaf))" }; continue }
    if (-not $DryRun) { Copy-IntoOverlay $from $Dest $p.Dll $Protected }
    $rows += [pscustomobject]@{ Dll = $p.Dll; Action = 'stage'; Source = $from; Why = $p.Crate }
    $companions = @($p.Companions)
    foreach ($fw in Get-PeSysForwarders $from) {
      if (-not ($companions | Where-Object { $_.Name -ieq $fw.Name })) { $companions += $fw }
    }
    foreach ($c in $companions) {
      if ($explicitSet.ContainsKey($c.Name.ToLower())) { $rows += [pscustomobject]@{ Dll = $c.Name; Action = 'script'; Source = $c.From; Why = 'staged by this run script' }; continue }
      if ($c.Kind -eq 'console' -and $c.From -and (Test-Path (Split-Path $c.From -Parent)) -and -not (Test-Path $c.From)) { $rows += [pscustomobject]@{ Dll = $c.Name; Action = 'skip'; Source = $c.From; Why = "title dump has no $(Split-Path $c.From -Leaf); console companion of $($p.Dll) not staged" }; continue }
      if (-not $c.From -or -not (Test-Path $c.From)) { $rows += [pscustomobject]@{ Dll = $c.Name; Action = 'missing'; Source = $c.From; Why = "$($c.Kind) companion of $($p.Dll) not found" }; continue }
      if (-not $DryRun) { Copy-IntoOverlay $c.From $Dest $c.Name $Protected }
      $rows += [pscustomobject]@{ Dll = $c.Name; Action = 'stage'; Source = $c.From; Why = "$($c.Kind) companion of $($p.Dll)" }
    }
  }
  [pscustomobject]@{ OsBuild = $os; Rows = $rows }
}

function Write-CompatStaging($Result, [string]$Dest, [switch]$DryRun) {
  $mode = if ($DryRun) { 'dry run, nothing written' } else { 'staged' }
  Write-Host "  compat DLLs -> $Dest (console OS $($Result.OsBuild); $mode)" -ForegroundColor DarkGray
  foreach ($r in $Result.Rows) { Write-Host ("    {0,-8} {1,-40} {2} [{3}]" -f $r.Action, $r.Dll, $r.Source, $r.Why) -ForegroundColor DarkGray }
}
