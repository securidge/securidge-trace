# Windows 가상 머신(GitHub Actions)에서 "정답을 아는" 실제 기록을 만든다.
#
# 이 스크립트가 하는 일
#   1. 시험 문서를 만들고, Windows 셸 API(SHAddToRecentDocs)로 "최근 연 파일" 바로 가기를 만들게 한다.
#      (탐색기가 파일을 열 때 부르는 것과 같은 API다.)
#   2. 별도 볼륨(VHD)을 만들어 그 안의 파일도 같은 방법으로 등록한다 → 볼륨 시리얼이 다른 바로 가기.
#   3. 파일을 휴지통으로 보낸다 → 실제 $I 파일.
#   4. Chrome·Edge를 실행해 표식이 붙은 주소를 방문한다 → 실제 History DB.
#   5. 각 행동의 정답(경로, 크기, 시각 범위, 볼륨 시리얼)을 truth.json에 적는다.
#
# 한계: 실제 USB(이동식 드라이브)는 만들 수 없다. VHD는 "고정 디스크"로 보인다.
#       GitHub의 Windows는 Windows Server다 (클라이언트 Windows 10/11과 다를 수 있다).

param(
    [Parameter(Mandatory = $true)][string]$TruthPath,
    [Parameter(Mandatory = $true)][string]$SampleDir
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $SampleDir | Out-Null

function UtcNow { [DateTime]::UtcNow.ToString('o') }

function Get-VolumeSerial([string]$Letter) {
    $v = Get-CimInstance Win32_Volume -Filter "DriveLetter='$($Letter):'"
    if (-not $v) { return $null }
    $s = [uint32]$v.SerialNumber
    return ('{0:X4}-{1:X4}' -f ($s -shr 16), ($s -band 0xFFFF))
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

public static class StShell {
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern void SHAddToRecentDocs(uint uFlags, string pv);

    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    class ShellLinkCoClass { }

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown),
     Guid("000214F9-0000-0000-C000-000000000046")]
    interface IShellLinkW {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder f, int cch, IntPtr fd, uint fl);
        void GetIDList(out IntPtr ppidl);
        void SetIDList(IntPtr pidl);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder n, int cch);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string n);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder d, int cch);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string d);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder a, int cch);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string a);
        void GetHotkey(out short h);
        void SetHotkey(short h);
        void GetShowCmd(out int c);
        void SetShowCmd(int c);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder p, int cch, out int i);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string p, int i);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string p, uint r);
        void Resolve(IntPtr hwnd, uint fl);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string f);
    }

    // Windows 셸의 유니코드 링크 API로 바로 가기를 만든다 (탐색기와 같은 구성 요소).
    public static void CreateLink(string target, string lnkPath) {
        var link = (IShellLinkW)new ShellLinkCoClass();
        link.SetPath(target);
        ((IPersistFile)link).Save(lnkPath, true);
    }
}
'@

$truth = [ordered]@{
    os          = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber)
    time_zone   = (Get-TimeZone).Id
    account     = $env:USERNAME
    sid         = ([System.Security.Principal.WindowsIdentity]::GetCurrent()).User.Value
    recent_dir  = [Environment]::GetFolderPath('Recent')
    opened      = @()
    deleted     = @()
    web         = @()
    notes       = @()
}

$recent = $truth.recent_dir

function Register-Opened([string]$Path, [string]$Letter, [string]$Label) {
    $before = UtcNow
    [StShell]::SHAddToRecentDocs(3, $Path)   # 3 = SHARD_PATHW
    Start-Sleep -Seconds 3
    $after = UtcNow
    $name = [IO.Path]::GetFileName($Path)
    $lnk = Join-Path $recent "$name.lnk"
    $method = 'SHAddToRecentDocs'
    if (-not (Test-Path -LiteralPath $lnk)) {
        # 셸이 바로 가기를 만들지 않은 경우(서버 Windows는 최근 문서 기록이 꺼져 있음):
        # 같은 셸 구성 요소(IShellLinkW)로 Recent 폴더에 직접 만든다.
        New-Item -ItemType Directory -Force -Path $recent | Out-Null
        [StShell]::CreateLink($Path, $lnk)
        $after = UtcNow
        $method = 'IShellLinkW (fallback: SHAddToRecentDocs가 만들지 않음)'
    }
    Copy-Item -LiteralPath $lnk -Destination (Join-Path $SampleDir "$Label.lnk")
    $script:truth.opened += [ordered]@{
        label         = $Label
        target        = $Path
        size          = (Get-Item -LiteralPath $Path).Length
        volume_serial = (Get-VolumeSerial $Letter)
        lnk           = $lnk
        method        = $method
        time_min_utc  = $before
        time_max_utc  = $after
    }
}

# --- 1. 시험 문서 (한글 이름 포함) ---
$docs = Join-Path $env:USERPROFILE 'Documents\시험문서'
New-Item -ItemType Directory -Force -Path $docs | Out-Null
$f1 = Join-Path $docs '견적서_시험.txt'
$f2 = Join-Path $docs 'memo_test.txt'
Set-Content -LiteralPath $f1 -Value ('A' * 1234) -NoNewline -Encoding utf8NoBOM
Set-Content -LiteralPath $f2 -Value ('B' * 77) -NoNewline -Encoding utf8NoBOM
Start-Sleep -Seconds 2
function Try-Step([string]$Name, [scriptblock]$Body) {
    try { & $Body } catch { $script:truth.notes += "$Name 실패: $($_.Exception.Message)"; Write-Warning "$Name 실패: $_" }
}
$policy = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer' -ErrorAction SilentlyContinue
$truth.notes += "NoRecentDocsHistory 정책 값: $($policy.NoRecentDocsHistory)"
Try-Step 'opened_korean_c' { Register-Opened $f1 'C' 'opened_korean_c' }
Start-Sleep -Seconds 2
Try-Step 'opened_ascii_c' { Register-Opened $f2 'C' 'opened_ascii_c' }

# --- 2. 별도 볼륨 (VHD) ---
try {
    $vhd = 'D:\st-testvol.vhdx'
    $dp = @"
create vdisk file="$vhd" maximum=256 type=expandable
select vdisk file="$vhd"
attach vdisk
create partition primary
format fs=ntfs label="STTEST" quick
assign letter=T
"@
    $dpFile = 'D:\st-diskpart.txt'
    Set-Content -LiteralPath $dpFile -Value $dp -Encoding ascii
    diskpart /s $dpFile | Out-Host
    Start-Sleep -Seconds 3
    if (-not (Test-Path 'T:\')) { throw 'T: 드라이브가 생기지 않음' }
    New-Item -ItemType Directory -Force -Path 'T:\자료' | Out-Null
    $f3 = 'T:\자료\usb문서_시험.txt'
    Set-Content -LiteralPath $f3 -Value ('C' * 4096) -NoNewline -Encoding utf8NoBOM
    Start-Sleep -Seconds 2
    Try-Step 'opened_korean_t' { Register-Opened $f3 'T' 'opened_korean_t' }
} catch {
    $truth.notes += "별도 볼륨 시험 건너뜀: $($_.Exception.Message)"
}

# --- 3. 휴지통 ---
Add-Type -AssemblyName Microsoft.VisualBasic
function Send-ToBin([string]$Path, [string]$Label) {
    $size = (Get-Item -LiteralPath $Path).Length
    $before = UtcNow
    [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile(
        $Path, 'OnlyErrorDialogs', 'SendToRecycleBin')
    $after = UtcNow
    $script:truth.deleted += [ordered]@{
        label = $Label; original_path = $Path; size = $size
        time_min_utc = $before; time_max_utc = $after
    }
}
$d1 = Join-Path $docs '계약서 초안_시험.txt'
$d2 = Join-Path $docs 'delete_me.bin'
Set-Content -LiteralPath $d1 -Value ('D' * 5555) -NoNewline -Encoding utf8NoBOM
[IO.File]::WriteAllBytes($d2, [byte[]](1..200 | ForEach-Object { [byte]($_ % 256) }))
Start-Sleep -Seconds 2
Try-Step 'deleted_korean' { Send-ToBin $d1 'deleted_korean' }
Start-Sleep -Seconds 2
Try-Step 'deleted_ascii' { Send-ToBin $d2 'deleted_ascii' }

# 만들어진 $I 파일을 샘플로 보관 (나중에 회귀 시험 데이터로 쓴다)
$binDir = Join-Path 'C:\$Recycle.Bin' $truth.sid
Get-ChildItem -LiteralPath $binDir -Force -Filter '$I*' -ErrorAction SilentlyContinue |
    ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $SampleDir $_.Name) }

# --- 4. 브라우저 ---
$browsers = @(
    @{ name = 'Chrome'; exe = "$env:ProgramFiles\Google\Chrome\Application\chrome.exe"; proc = 'chrome' },
    @{ name = 'Edge'; exe = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"; proc = 'msedge' }
)
foreach ($b in $browsers) {
    if (-not (Test-Path -LiteralPath $b.exe)) {
        $truth.notes += "$($b.name) 없음"
        continue
    }
    $marker = "https://example.com/?st_marker=$($b.name.ToLower())-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
    $before = UtcNow
    $p = Start-Process -FilePath $b.exe -PassThru -ArgumentList @(
        '--headless=new', '--no-first-run', '--no-default-browser-check', '--disable-gpu', $marker)
    Start-Sleep -Seconds 25
    Get-Process -Name $b.proc -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 3
    $after = UtcNow
    $truth.web += [ordered]@{
        browser = $b.name; url = $marker; time_min_utc = $before; time_max_utc = $after
    }
}

$truth | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $TruthPath -Encoding utf8NoBOM
Write-Host "정답 파일: $TruthPath"
Get-Content -LiteralPath $TruthPath | Write-Host
