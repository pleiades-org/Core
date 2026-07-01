Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

<#
.SYNOPSIS
    Generates a multi-resolution app_icon.ico from assets\Core.png.

.DESCRIPTION
    Reads the Core logo PNG (512×512) and writes a Windows ICO file
    containing six resolutions (16, 32, 48, 64, 128, 256) using
    Vista-compatible PNG-compressed entries.
#>

Add-Type -AssemblyName System.Drawing

$repositoryRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')
$sourceImagePath = Join-Path $repositoryRoot 'assets\Core.png'
$outputIconPath = Join-Path $repositoryRoot 'assets\app_icon.ico'

if (-not (Test-Path -LiteralPath $sourceImagePath)) {
    throw "Source image not found: $sourceImagePath"
}

$sizes = @(256, 128, 64, 48, 32, 16)
$pngBytesList = New-Object System.Collections.ArrayList

foreach ($size in $sizes) {
    $sourceImage = [System.Drawing.Image]::FromFile($sourceImagePath)
    try {
        $bitmap = New-Object System.Drawing.Bitmap($size, $size)
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
                $graphics.DrawImage($sourceImage, 0, 0, $size, $size)
            } finally {
                $graphics.Dispose()
            }

            $pngStream = New-Object System.IO.MemoryStream
            try {
                $bitmap.Save($pngStream, [System.Drawing.Imaging.ImageFormat]::Png)
                $null = $pngBytesList.Add($pngStream.ToArray())
            } finally {
                $pngStream.Dispose()
            }
        } finally {
            $bitmap.Dispose()
        }
    } finally {
        $sourceImage.Dispose()
    }
}

$iconStream = New-Object System.IO.MemoryStream
try {
    $writer = New-Object System.IO.BinaryWriter($iconStream)
    try {
        # ICO Header (6 bytes)
        $writer.Write([UInt16]0)                          # Reserved
        $writer.Write([UInt16]1)                          # Type: ICO
        $writer.Write([UInt16]$sizes.Count)                # Image count

        # Calculate data offset: header(6) + entries(16 each)
        $dataOffset = 6 + (16 * $sizes.Count)

        for ($i = 0; $i -lt $sizes.Count; $i++) {
            $size = $sizes[$i]
            $pngData = $pngBytesList[$i]
            $pngLength = $pngData.Length

            # Directory Entry (16 bytes)
            $writer.Write([byte]$(if ($size -ge 256) { 0 } else { $size }))   # Width  (0 = 256)
            $writer.Write([byte]$(if ($size -ge 256) { 0 } else { $size }))   # Height (0 = 256)
            $writer.Write([byte]0)                                             # Color palette
            $writer.Write([byte]0)                                             # Reserved
            $writer.Write([UInt16]1)                                           # Color planes
            $writer.Write([UInt16]32)                                          # Bits per pixel
            $writer.Write([UInt32]$pngLength)                                  # Data size
            $writer.Write([UInt32]$dataOffset)                                # Data offset

            $dataOffset += $pngLength
        }

        # Write PNG data for each resolution
        for ($i = 0; $i -lt $sizes.Count; $i++) {
            $writer.Write([byte[]]$pngBytesList[$i])
        }
    } finally {
        $writer.Dispose()
    }

    [System.IO.File]::WriteAllBytes($outputIconPath, $iconStream.ToArray())
} finally {
    $iconStream.Dispose()
}

$iconFile = Get-Item -LiteralPath $outputIconPath
Write-Host "Created $outputIconPath ($($iconFile.Length) bytes) with sizes: $($sizes -join ', ')"