# QuickPaste → QuickCopy 图标改色：金色 (239,179,54) → #DB7A0E (219,122,14)
Add-Type -AssemblyName System.Drawing

function Test-Gold($c) {
    return $c.R -ge 170 -and $c.G -ge 105 -and $c.G -gt $c.B -and ($c.R - $c.B) -gt 55 -and $c.B -lt 190 -and $c.A -gt 0
}

function Recolor-Png($path) {
    $tmp = $path + '.tmp'
    $src = [System.Drawing.Image]::FromFile($path)
    $bmp = New-Object System.Drawing.Bitmap($src)   # 复制像素，随后释放源句柄
    $src.Dispose()
    for ($y = 0; $y -lt $bmp.Height; $y++) {
        for ($x = 0; $x -lt $bmp.Width; $x++) {
            $c = $bmp.GetPixel($x, $y)
            if (Test-Gold $c) {
                $r = [Math]::Min(255, $c.R - 20)
                $g = [Math]::Max(0, $c.G - 57)
                $b = [Math]::Max(0, $c.B - 40)
                $bmp.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($c.A, $r, $g, $b))
            }
        }
    }
    $bmp.Save($tmp, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Move-Item -Force $tmp $path
    Write-Host "recolored: $path"
}

function Build-Ico($pngPath, $icoPath) {
    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $blobs = @()
    $src = [System.Drawing.Image]::FromFile($pngPath)
    $bmp0 = New-Object System.Drawing.Bitmap($src)
    $src.Dispose()
    foreach ($s in $sizes) {
        $b = [System.Drawing.Bitmap]::new($bmp0, $s, $s)
        $ms = New-Object System.IO.MemoryStream
        $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $blobs += ,$ms.ToArray()
        $ms.Dispose(); $b.Dispose()
    }
    $bmp0.Dispose()
    $fs = [System.IO.File]::Create($icoPath)
    $w = New-Object System.IO.BinaryWriter($fs)
    $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $s = $sizes[$i]
        $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
        $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
        $w.Write([byte]0); $w.Write([byte]0)
        $w.Write([uint16]1); $w.Write([uint16]32)
        $w.Write([uint32]$blobs[$i].Length)
        $w.Write([uint32]$offset)
        $offset += $blobs[$i].Length
    }
    foreach ($blob in $blobs) { $w.Write($blob) }
    $w.Close(); $fs.Close()
    Write-Host "ico rebuilt: $icoPath"
}

$pcIcons = @(
    'E:\2Monkcode\1QuickPaste\src-tauri\icons\icon.png',
    'E:\2Monkcode\1QuickPaste\src-tauri\icons\128x128.png',
    'E:\2Monkcode\1QuickPaste\src-tauri\icons\128x128@2x.png',
    'E:\2Monkcode\1QuickPaste\src-tauri\icons\32x32.png'
)
$mipmap = @(
    'E:\2Monkcode\1QuickPaste\mobile-app\android\app\src\main\res\mipmap-mdpi\ic_launcher.png',
    'E:\2Monkcode\1QuickPaste\mobile-app\android\app\src\main\res\mipmap-hdpi\ic_launcher.png',
    'E:\2Monkcode\1QuickPaste\mobile-app\android\app\src\main\res\mipmap-xhdpi\ic_launcher.png',
    'E:\2Monkcode\1QuickPaste\mobile-app\android\app\src\main\res\mipmap-xxhdpi\ic_launcher.png',
    'E:\2Monkcode\1QuickPaste\mobile-app\android\app\src\main\res\mipmap-xxxhdpi\ic_launcher.png'
)
foreach ($p in $pcIcons) { Recolor-Png $p }
foreach ($p in $mipmap) { Recolor-Png $p }
Build-Ico 'E:\2Monkcode\1QuickPaste\src-tauri\icons\icon.png' 'E:\2Monkcode\1QuickPaste\src-tauri\icons\icon.ico'
Write-Host 'done'
