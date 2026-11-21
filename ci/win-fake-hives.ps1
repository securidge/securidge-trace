# 정답을 아는 레지스트리 하이브를 Windows에서 직접 만든다 (reg save).
#
# 가상 머신에는 USB·스마트폰·블루투스가 없으므로, 실제 Windows가 기록하는 것과 같은 키 구조에
# 알려진 값을 넣고 하이브 파일로 저장한다. 하이브 형식 자체는 Windows가 만든 진짜다.
# 결과: $Out\Windows\System32\config\SYSTEM, SOFTWARE 와 정답 파일 $Out\fake-truth.json

param([Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = 'Stop'

function Reg([string[]]$a) {
    & reg.exe @a | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "reg 실패: $($a -join ' ')" }
}
function FtHex([string]$iso) {
    $ft = [DateTime]::Parse($iso, $null, 'AdjustToUniversal,AssumeUniversal').ToFileTimeUtc()
    ([BitConverter]::GetBytes([long]$ft) | ForEach-Object { $_.ToString('x2') }) -join ''
}
function SysTimeHex([int[]]$v) {   # 년, 월, 요일, 일, 시, 분, 초, 밀리초
    ($v | ForEach-Object { ([BitConverter]::GetBytes([uint16]$_) | ForEach-Object { $_.ToString('x2') }) -join '' }) -join ''
}
function Utf16Hex([string]$s) {
    ([Text.Encoding]::Unicode.GetBytes($s) | ForEach-Object { $_.ToString('x2') }) -join ''
}

$S = 'HKCU\Software\STFakeSystem'
$W = 'HKCU\Software\STFakeSoftware'
reg delete $S /f 2>$null | Out-Null
reg delete $W /f 2>$null | Out-Null

# --- SYSTEM ---
Reg @('add', "$S\Select", '/v', 'Current', '/t', 'REG_DWORD', '/d', '1', '/f')
Reg @('add', "$S\ControlSet001\Control\ComputerName\ComputerName", '/v', 'ComputerName', '/t', 'REG_SZ', '/d', 'FAKE-PC-01', '/f')
Reg @('add', "$S\ControlSet001\Control\TimeZoneInformation", '/v', 'TimeZoneKeyName', '/t', 'REG_SZ', '/d', 'Korea Standard Time', '/f')
# Bias = -540 (DWORD 0xFFFFFDE4)
Reg @('add', "$S\ControlSet001\Control\TimeZoneInformation", '/v', 'Bias', '/t', 'REG_DWORD', '/d', '4294966756', '/f')
Reg @('add', "$S\ControlSet001\Control\Nls\CodePage", '/v', 'ACP', '/t', 'REG_SZ', '/d', '949', '/f')

$usbClass = 'Disk&Ven_SanDisk&Prod_Ultra_USB&Rev_1.00'
$usbInst = '4C530001230606104182&0'
$usbKey = "$S\ControlSet001\Enum\USBSTOR\$usbClass\$usbInst"
Reg @('add', $usbKey, '/v', 'FriendlyName', '/t', 'REG_SZ', '/d', 'SanDisk Ultra USB Device', '/f')
$props = "$usbKey\Properties\{83da6326-97a6-4088-9453-a1923f573b29}"
Reg @('add', "$props\0064", '/ve', '/t', 'REG_BINARY', '/d', (FtHex '2026-11-03T13:00:00Z'), '/f')
Reg @('add', "$props\0066", '/ve', '/t', 'REG_BINARY', '/d', (FtHex '2026-11-03T13:12:00Z'), '/f')
Reg @('add', "$props\0067", '/ve', '/t', 'REG_BINARY', '/d', (FtHex '2026-11-03T13:40:00Z'), '/f')

# Windows가 고유 번호를 만들어 붙인 장치 (두 번째 글자가 '&')
$usb2 = "$S\ControlSet001\Enum\USBSTOR\Disk&Ven_Generic&Prod_Flash_Disk&Rev_8.07\7&2a8c3f1&0"
Reg @('add', $usb2, '/v', 'FriendlyName', '/t', 'REG_SZ', '/d', 'Generic Flash Disk USB Device', '/f')

$mtp = "$S\ControlSet001\Enum\USB\VID_04E8&PID_6860\R58M123ABC"
Reg @('add', $mtp, '/v', 'ClassGUID', '/t', 'REG_SZ', '/d', '{eec5ad98-8080-425f-922a-dabf3de3f69a}', '/f')
Reg @('add', $mtp, '/v', 'Mfg', '/t', 'REG_SZ', '/d', '@wpdmtp.inf,%samsung%;Samsung', '/f')
Reg @('add', $mtp, '/v', 'DeviceDesc', '/t', 'REG_SZ', '/d', '@wpdmtp.inf,%desc%;Galaxy S24', '/f')
Reg @('add', "$mtp\Properties\{83da6326-97a6-4088-9453-a1923f573b29}\0066", '/ve', '/t', 'REG_BINARY', '/d', (FtHex '2026-11-04T01:30:00Z'), '/f')
# 휴대 장치가 아닌 USB 장치 (키보드) — 목록에 나오면 안 된다
Reg @('add', "$S\ControlSet001\Enum\USB\VID_046D&PID_C31C\6&1234&0&1", '/v', 'ClassGUID', '/t', 'REG_SZ', '/d', '{4d36e96b-e325-11ce-bfc1-08002be10318}', '/f')

$bt = "$S\ControlSet001\Services\BTHPORT\Parameters\Devices\a0b1c2d3e4f5"
$btName = (([Text.Encoding]::ASCII.GetBytes('Galaxy Buds') + [byte]0) | ForEach-Object { $_.ToString('x2') }) -join ''
Reg @('add', $bt, '/v', 'Name', '/t', 'REG_BINARY', '/d', $btName, '/f')
$btLast = ([DateTime]::Parse('2026-11-05T09:00:00Z', $null, 'AdjustToUniversal,AssumeUniversal').ToFileTimeUtc()).ToString()
Reg @('add', $bt, '/v', 'LastConnected', '/t', 'REG_QWORD', '/d', $btLast, '/f')

$md = '_??_USBSTOR#' + $usbClass + '#' + $usbInst + '#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}'
Reg @('add', "$S\MountedDevices", '/v', '\DosDevices\E:', '/t', 'REG_BINARY', '/d', (Utf16Hex $md), '/f')
Reg @('add', "$S\MountedDevices", '/v', '\DosDevices\C:', '/t', 'REG_BINARY', '/d', 'e0041f53000010000000000000', '/f')

# --- SOFTWARE ---
$guid = '{11111111-2222-3333-4444-555555555555}'
$np = "$W\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles\$guid"
Reg @('add', $np, '/v', 'ProfileName', '/t', 'REG_SZ', '/d', 'Cafe-WiFi', '/f')
Reg @('add', $np, '/v', 'NameType', '/t', 'REG_DWORD', '/d', '71', '/f')
# 로컬 2026-11-03(화) 22:12:05, 2026-11-04(수) 08:00:00
Reg @('add', $np, '/v', 'DateCreated', '/t', 'REG_BINARY', '/d', (SysTimeHex @(2026, 11, 2, 3, 22, 12, 5, 0)), '/f')
Reg @('add', $np, '/v', 'DateLastConnected', '/t', 'REG_BINARY', '/d', (SysTimeHex @(2026, 11, 3, 4, 8, 0, 0, 0)), '/f')
$sig = "$W\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Unmanaged\0102030405"
Reg @('add', $sig, '/v', 'ProfileGuid', '/t', 'REG_SZ', '/d', $guid, '/f')
Reg @('add', $sig, '/v', 'DefaultGatewayMac', '/t', 'REG_BINARY', '/d', '001122aabbcc', '/f')
$pl = "$W\Microsoft\Windows NT\CurrentVersion\ProfileList\S-1-5-21-1111-2222-3333-1001"
Reg @('add', $pl, '/v', 'ProfileImagePath', '/t', 'REG_EXPAND_SZ', '/d', 'C:\Users\김철수', '/f')
Reg @('add', "$W\Microsoft\Windows NT\CurrentVersion", '/v', 'ProductName', '/t', 'REG_SZ', '/d', 'Windows 11 Pro', '/f')
Reg @('add', "$W\Microsoft\Windows NT\CurrentVersion", '/v', 'CurrentBuild', '/t', 'REG_SZ', '/d', '26100', '/f')

$cfg = Join-Path $Out 'Windows\System32\config'
New-Item -ItemType Directory -Force $cfg | Out-Null
Reg @('save', $S, (Join-Path $cfg 'SYSTEM'), '/y')
Reg @('save', $W, (Join-Path $cfg 'SOFTWARE'), '/y')
reg delete $S /f | Out-Null
reg delete $W /f | Out-Null

@{
    computer_name = 'FAKE-PC-01'; time_zone = 'Korea Standard Time'; utc_offset_minutes = 540; codepage = 949
    usb = @{ vendor = 'SanDisk'; product = 'Ultra USB'; serial = '4C530001230606104182'; unique = $true
             first = '2026-11-03T13:00:00Z'; last = '2026-11-03T13:12:00Z'; removed = '2026-11-03T13:40:00Z'; letters = @('E:') }
    usb2 = @{ serial = '7&2a8c3f1&0'; unique = $false }
    portable = @{ vendor = 'Samsung'; product = 'Galaxy S24'; serial = 'R58M123ABC'; last = '2026-11-04T01:30:00Z' }
    bluetooth = @{ name = 'Galaxy Buds'; serial = 'A0:B1:C2:D3:E4:F5'; last = '2026-11-05T09:00:00Z' }
    network = @{ name = 'Cafe-WiFi'; medium = 'wireless'; created_utc = '2026-11-03T13:12:05Z'; last_utc = '2026-11-03T23:00:00Z'; gateway = '00:11:22:AA:BB:CC' }
    account = @{ sid = 'S-1-5-21-1111-2222-3333-1001'; name = '김철수' }
    product = 'Windows 11 Pro'; build = '26100'
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8NoBOM (Join-Path $Out 'fake-truth.json')
Write-Host "정답 하이브를 만들었습니다: $cfg"
