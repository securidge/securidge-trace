# 시연 캡처: 공개된 릴리스를 받아 Windows 가상 머신에서 실제로 점검을 돌리고 화면을 찍는다.
# 시연용 상황: 보안·시스템 로그를 지우고 시계를 20분 바꿨다가 되돌린다 (win-live-setup.ps1).
param([string]$Out = 'D:\demo')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null

try { Set-DisplayResolution -Width 1600 -Height 900 -Force -ErrorAction Stop } catch { Write-Host "해상도 변경 안 됨: $_" }

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref PT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
}
"@

function Shot($hwnd, $name) {
    $r = New-Object W+RECT; [W]::GetClientRect($hwnd, [ref]$r) | Out-Null
    $p = New-Object W+PT; [W]::ClientToScreen($hwnd, [ref]$p) | Out-Null
    $w = $r.R - $r.L; $h = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p.X, $p.Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
    $file = Join-Path $Out "$name.png"
    $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "찍음: $name ($w x $h)"
}

function Click($hwnd, $x, $y) {
    $p = New-Object W+PT; $p.X = $x; $p.Y = $y
    [W]::ClientToScreen($hwnd, [ref]$p) | Out-Null
    [W]::SetCursorPos($p.X, $p.Y) | Out-Null
    Start-Sleep -Milliseconds 300
    [W]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80
    [W]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 700
}

# 1) 공개 릴리스 내려받기
$zip = Join-Path $Out 'st.zip'
Invoke-WebRequest 'https://github.com/securidge/securidge-trace/releases/download/v0.2.0/SecuridgeTrace-v0.2.0-windows-x64.zip' -OutFile $zip
Expand-Archive $zip -DestinationPath (Join-Path $Out 'app') -Force
$exe = Get-ChildItem (Join-Path $Out 'app') -Recurse -Filter SecuridgeTrace.exe | Select-Object -First 1
Write-Host "실행 파일: $($exe.FullName)"

# 2) 시연 상황 만들기 (로그 삭제·시계 변경)
& "$PSScriptRoot\win-live-setup.ps1" -Truth (Join-Path $Out 'truth.json')

# 3) 처음 실행 설정 (언어·안내 확인은 건너뜀), 회사 이름
$env:ST_DATA_DIR = Join-Path $Out 'data'
New-Item -ItemType Directory -Force $env:ST_DATA_DIR | Out-Null
'{"ui_lang":"ko","report_lang":"ko","consent_accepted":true,"analyst":"홍길동","theme":"dark"}' | Set-Content -Encoding utf8NoBOM (Join-Path $env:ST_DATA_DIR 'settings.json')
'[{"name":"시연 회사"}]' | Set-Content -Encoding utf8NoBOM (Join-Path $env:ST_DATA_DIR 'customers.json')

$proc = Start-Process -FilePath $exe.FullName -PassThru
Start-Sleep -Seconds 8
$proc.Refresh()
$hwnd = $proc.MainWindowHandle
Write-Host "창 핸들: $hwnd"
[W]::ShowWindow($hwnd, 9) | Out-Null
[W]::MoveWindow($hwnd, 0, 0, 1116, 799, $true) | Out-Null
[W]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Seconds 2
Shot $hwnd '01_home'

Click $hwnd 407 249          # 원형 [점검 시작]
Start-Sleep -Seconds 2
Shot $hwnd '02_form'
Click $hwnd 282 432          # 권한 확인 체크
Start-Sleep -Seconds 1
Shot $hwnd '03_form_checked'
Click $hwnd 486 510          # [점검 시작]
Start-Sleep -Seconds 3
Shot $hwnd '04_progress'
Start-Sleep -Seconds 12
Shot $hwnd '05_progress2'

# 끝날 때까지 기다린다 (지난 내역 폴더에 보고서가 생기면 끝)
$hist = Join-Path $env:ST_DATA_DIR 'history'
for ($i = 0; $i -lt 60; $i++) {
    $rep = Get-ChildItem $hist -Recurse -Filter report_ko.html -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($rep) { break }
    Start-Sleep -Seconds 5
}
Start-Sleep -Seconds 4
Shot $hwnd '06_result'
Click $hwnd 110 180          # 왼쪽 메뉴 [지난 내역]
Start-Sleep -Seconds 2
Shot $hwnd '07_history'
Stop-Process -Id $proc.Id -Force

# 4) 보고서를 Edge로 찍기
$edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($rep -and $edge) {
    $url = 'file:///' + ($rep.FullName -replace '\\', '/')
    & $edge --headless=new --disable-gpu --hide-scrollbars --window-size=1200,2600 "--screenshot=$(Join-Path $Out '08_report.png')" $url | Out-Null
    Start-Sleep -Seconds 3
    Copy-Item $rep.FullName (Join-Path $Out 'report_ko.html')
    $case = Join-Path $rep.Directory.FullName 'case.json'
    $c = Get-Content $case -Raw -Encoding utf8 | ConvertFrom-Json
    Write-Host "등급: $($c.assessment.grade), 기록: $($c.analysis.events.Count), 판단: $(($c.assessment.findings | ForEach-Object { $_.rule_id }) -join ',')"
}

# 5) 결과물 다운로드가 막힌 환경용: 그림을 로그로 내보낸다
Get-ChildItem $Out -Filter *.png | Sort-Object Name | ForEach-Object {
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($_.FullName))
    Write-Host "ST-IMG-BEGIN $($_.Name)"
    for ($i = 0; $i -lt $b64.Length; $i += 4000) { Write-Host $b64.Substring($i, [Math]::Min(4000, $b64.Length - $i)) }
    Write-Host "ST-IMG-END"
}
