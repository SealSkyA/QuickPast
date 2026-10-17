# import-build-loop.ps1 - iterate: cargo build -> parse missing import libs -> generate -> rebuild until green
$ErrorActionPreference = 'Continue'
$log = "$env:TEMP\qpp-import-loop.log"
$st = "$env:TEMP\qpp-import-loop-status.txt"
$work = 'E:\2Monkcode\1QuickPaste\src-tauri'
$gen  = 'E:\2Monkcode\1QuickPaste\tools\gen-mingw-imports.ps1'
$shim = 'E:\2Monkcode\toolchains\mingw-shims'
$env:PATH = "$shim;$env:USERPROFILE\.cargo\bin;$env:PATH"
Set-Location $work
Remove-Item $log, $st -ErrorAction SilentlyContinue

"loop start $(Get-Date -Format 'HH:mm:ss')" | Add-Content $log

for ($iter = 1; $iter -le 30; $iter++) {
    "===== iteration $iter =====" | Add-Content $log
    $bl = "$env:TEMP\qpp-cargo-build-$iter.log"
    $inner = 'rustup run stable-x86_64-pc-windows-gnu cargo build > "' + $bl + '" 2>&1'
    cmd /c $inner
    $code = $LASTEXITCODE
    "build exit=$code" | Add-Content $log
    if ($code -eq 0) {
        "BUILD SUCCESS" | Add-Content $log
        "SUCCESS" | Set-Content $st
        break
    }
    $missing = @()
    Select-String -Path $bl -Pattern 'unable to find library -l[:a-zA-Z0-9_.-]+|cannot find -l[:a-zA-Z0-9_.-]+' -AllMatches -ErrorAction SilentlyContinue |
        ForEach-Object { $_.Matches.Value } |
        ForEach-Object { if ($_ -match '-l([:a-zA-Z0-9_.-]+)') { $matches[1] } } |
        ForEach-Object {
            $n = $_
            if ($n.StartsWith(':')) { $n = $n.Substring(1) }
            if ($n -match '^lib(.+)\.a$') { $n = $matches[1] }
            $n
        } | Sort-Object -Unique | ForEach-Object { $missing += $_ }
    if (-not $missing) {
        "NO MISSING LIBS but build failed - check $bl" | Add-Content $log
        Get-Content $bl -Tail 20 | ForEach-Object { "  $_" | Add-Content $log }
        "FAILED_OTHER" | Set-Content $st
        break
    }
    "missing libs: $($missing -join ', ')" | Add-Content $log
    $skip = @('gcc_eh','pthread','gcc','mingwex','mingw32','msvcrt','winpthread')
    $toGen = $missing | Where-Object { $skip -notcontains $_ }
    $skipHit = $missing | Where-Object { $skip -contains $_ }
    if ($skipHit) { "skipped special libs: $($skipHit -join ', ')" | Add-Content $log }
    if ($toGen) {
        powershell -NoProfile -ExecutionPolicy Bypass -File $gen -Libs $toGen 2>&1 | ForEach-Object { "  $_" | Add-Content $log }
    }
}
"DONE" | Add-Content $log