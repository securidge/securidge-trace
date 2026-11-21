# 화면 프로그램이 Windows에서 뜨는지 확인하고 스크린샷을 남긴다.
param([Parameter(Mandatory = $true)][string]$Exe, [Parameter(Mandatory = $true)][string]$Shot)
$ErrorActionPreference = 'Stop'
$env:ST_DATA_DIR = 'D:\st-gui-data'
New-Item -ItemType Directory -Force $env:ST_DATA_DIR | Out-Null
'{"ui_lang":"ko","consent_accepted":true}' | Set-Content -Encoding utf8NoBOM (Join-Path $env:ST_DATA_DIR 'settings.json')
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 15
$alive = -not $p.HasExited
Write-Host "15초 뒤 실행 중: $alive"
if (-not $alive) { Write-Host "종료 코드: $($p.ExitCode)" }
try {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $small = New-Object System.Drawing.Bitmap $bmp, ([int]($b.Width * 0.6)), ([int]($b.Height * 0.6))
    $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object MimeType -eq 'image/jpeg'
    $ep = New-Object System.Drawing.Imaging.EncoderParameters 1
    $ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 55L
    $small.Save($Shot, $codec, $ep)
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($Shot))
    Write-Host "ST-SHOT-BEGIN"
    for ($i = 0; $i -lt $b64.Length; $i += 1000) { Write-Host $b64.Substring($i, [Math]::Min(1000, $b64.Length - $i)) }
    Write-Host "ST-SHOT-END"
} catch { Write-Host "스크린샷 실패: $_" }
if ($alive) { Stop-Process -Id $p.Id -Force }
if (-not $alive) { exit 1 }
