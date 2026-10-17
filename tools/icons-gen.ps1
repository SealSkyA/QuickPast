# icons-gen.ps1 - generate Tauri icon set from a 1024px source PNG (pure .NET, no downloads).
param([string]$Source = 'E:\2Monkcode\1QuickPaste\src-tauri\icons\icon.png')
Add-Type -AssemblyName System.Drawing
$dir = Split-Path -Parent $Source
$src = [System.Drawing.Image]::FromFile($Source)

function Resize-Png([int]$size, [string]$outPath) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.SmoothingMode = 'HighQuality'
    $g.DrawImage($src, 0, 0, $size, $size)
    $g.Dispose()
    $bmp.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

# standard sizes referenced by default tauri templates
Resize-Png 32   (Join-Path $dir '32x32.png')
Resize-Png 128  (Join-Path $dir '128x128.png')
Resize-Png 256  (Join-Path $dir '128x128@2x.png')

# build icon.ico with PNG payloads (Vista+ ICO v3)
$sizes = 16, 32, 48, 64, 128, 256
$pngs = @()
foreach ($s in $sizes) {
    $tmp = Join-Path $env:TEMP "qpp-ico-$s.png"
    Resize-Png $s $tmp
    $bytes = [System.IO.File]::ReadAllBytes($tmp)
    $pngs += , @{ Size = $s; Data = $bytes }
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
}

$ico = Join-Path $dir 'icon.ico'
$fs = [System.IO.File]::Create($ico)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([UInt16]0)                    # reserved
$bw.Write([UInt16]1)                    # type: icon
$bw.Write([UInt16]$pngs.Count)          # count
$offset = 6 + 16 * $pngs.Count
foreach ($p in $pngs) {
    $w = if ($p.Size -ge 256) { 0 } else { $p.Size }
    $bw.Write([byte]$w)                 # width (0 = 256)
    $bw.Write([byte]$w)                 # height
    $bw.Write([byte]0)                  # colors
    $bw.Write([byte]0)                  # reserved
    $bw.Write([UInt16]1)                # planes
    $bw.Write([UInt16]32)               # bitcount
    $bw.Write([UInt32]$p.Data.Length)   # bytes in res
    $bw.Write([UInt32]$offset)          # image offset
    $offset += $p.Data.Length
}
foreach ($p in $pngs) { $bw.Write($p.Data) }
$bw.Close()
$fs.Close()
"icon.ico OK: $((Get-Item $ico).Length) bytes"
Get-ChildItem $dir | Select-Object Name, Length | Format-Table -AutoSize | Out-String