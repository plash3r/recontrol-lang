$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$output = Join-Path $PSScriptRoot '..\icons\recontrol.png'
$bitmap = New-Object System.Drawing.Bitmap(128, 128)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([System.Drawing.Color]::FromArgb(31, 41, 55))
$cyan = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(103, 232, 249))
$gold = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(251, 191, 36), 8)
$font = New-Object System.Drawing.Font('Arial', 78, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$graphics.DrawString('R', $font, $cyan, 25, 20)
$graphics.DrawLine($gold, 30, 108, 98, 108)
$bitmap.Save($output, [System.Drawing.Imaging.ImageFormat]::Png)
$font.Dispose()
$cyan.Dispose()
$gold.Dispose()
$graphics.Dispose()
$bitmap.Dispose()
Write-Host "Created $output"
