# gen-mingw-imports.ps1 - generate GNU-style import libraries from system DLLs.
# Rationale: on windows-gnu rustc falls back to dlltool when an import lib is missing;
# we pre-generate every import lib rustc needs, so dlltool is never called.
# Usage: powershell -ExecutionPolicy Bypass -File gen-mingw-imports.ps1 -Libs user32,shell32,ws2_32
# Output: E:\2Monkcode\toolchains\mingw-imports\lib<name>.a

param(
    [Parameter(Mandatory = $true)][string[]]$Libs
)

# normalize: accept either a real array or a single comma-separated string
$Libs = @($Libs) | ForEach-Object { $_.Split(',') } | Where-Object { $_ }

$ErrorActionPreference = 'Stop'
$impDir = 'E:\2Monkcode\toolchains\mingw-imports'
$rustbin = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin"
$lld = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe"
$readobj = "$rustbin\llvm-readobj.exe"
New-Item -ItemType Directory -Force -Path $impDir | Out-Null

foreach ($lib in $Libs) {
    $out = Join-Path $impDir "lib$lib.a"
    if (Test-Path $out) { Write-Host "skip(exists) lib$lib.a"; continue }

    $dll = $null
    foreach ($ext in @('.dll', '.drv', '.sys')) {
        $cand = "C:\Windows\System32\$lib$ext"
        if (Test-Path $cand) { $dll = $cand; break }
    }
    if (-not $dll) { Write-Host "SKIP: no such system DLL: $lib"; continue }

    # 1) list named exports of the DLL
    $exports = & $readobj --coff-exports $dll 2>$null | Select-String 'Name:\s+([A-Za-z_@0-9$.]+)\s*$' | ForEach-Object {
        $_.Matches[0].Groups[1].Value
    }
    if (-not $exports) { Write-Host "WARN: no named exports parsed for $lib ($dll)"; continue }

    # 2) write .def
    $def = Join-Path $impDir "$lib.def"
    Set-Content -Path $def -Value "LIBRARY $lib" -Encoding ASCII
    Add-Content -Path $def -Value 'EXPORTS' -Encoding ASCII
    $exports | Sort-Object -Unique | ForEach-Object { Add-Content -Path $def -Value ("  " + $_) -Encoding ASCII }

    # 3) generate import library with lld-link
    $stub = Join-Path $impDir "stub_$lib.dll"
    & $lld -flavor link -def:"$def" -dll -noentry "-implib:$out" "-out:$stub" -machine:x64 2>&1 | ForEach-Object { Write-Host "lld($lib): $_" }
    if ($LASTEXITCODE -eq 0) {
        Write-Host ("OK   lib$lib.a  (" + $exports.Count + " exports)")
        Remove-Item $stub, $def -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "FAIL lib$lib.a"
    }
}