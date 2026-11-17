# Generates the spike's wallpaper (2560x1440 JPEG) and 20 icons (128x128 PNG) into assets\.
Add-Type -AssemblyName System.Drawing
$dir = Join-Path $PSScriptRoot 'assets'
$rnd = New-Object System.Random 42

$bmp = New-Object System.Drawing.Bitmap 2560, 1440
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$rect = New-Object System.Drawing.Rectangle 0, 0, 2560, 1440
$brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, ([System.Drawing.Color]::FromArgb(20, 30, 90)), ([System.Drawing.Color]::FromArgb(200, 80, 120)), 35.0
$g.FillRectangle($brush, $rect)
for ($i = 0; $i -lt 400; $i++) {
  $c = [System.Drawing.Color]::FromArgb(40, $rnd.Next(256), $rnd.Next(256), $rnd.Next(256))
  $r = $rnd.Next(10, 260)
  $b = New-Object System.Drawing.SolidBrush $c
  $g.FillEllipse($b, $rnd.Next(-100, 2560), $rnd.Next(-100, 1440), $r, $r)
}
$jpeg = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object MimeType -eq 'image/jpeg'
$ep = New-Object System.Drawing.Imaging.EncoderParameters 1
$ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 88L
$bmp.Save((Join-Path $dir 'wallpaper.jpg'), $jpeg, $ep)

$font = New-Object System.Drawing.Font 'Segoe UI', 56, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
$fmt = New-Object System.Drawing.StringFormat
$fmt.Alignment = 'Center'; $fmt.LineAlignment = 'Center'
for ($i = 0; $i -lt 20; $i++) {
  $ib = New-Object System.Drawing.Bitmap 128, 128, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $ig = [System.Drawing.Graphics]::FromImage($ib)
  $ig.SmoothingMode = 'AntiAlias'; $ig.TextRenderingHint = 'AntiAlias'
  $c = [System.Drawing.Color]::FromArgb(255, $rnd.Next(60, 240), $rnd.Next(60, 240), $rnd.Next(60, 240))
  $ig.FillEllipse((New-Object System.Drawing.SolidBrush $c), 6, 6, 116, 116)
  # thin rings and a glyph: fine detail to judge downscale crispness
  $ig.DrawEllipse((New-Object System.Drawing.Pen ([System.Drawing.Color]::White), 2), 14, 14, 100, 100)
  $ig.DrawEllipse((New-Object System.Drawing.Pen ([System.Drawing.Color]::Black), 1), 20, 20, 88, 88)
  $ig.DrawString([string][char](65 + $i), $font, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF 0, 0, 128, 128), $fmt)
  $ib.Save((Join-Path $dir ("icon{0:d2}.png" -f $i)), [System.Drawing.Imaging.ImageFormat]::Png)
}
