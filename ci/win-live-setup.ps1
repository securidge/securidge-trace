# 이 PC 점검 실기 검증 준비: 실제로 로그를 지우고 시계를 바꾼 뒤, PowerShell로 읽은 정답을 저장한다.
# GitHub의 일회용 가상 머신에서만 실행할 것 (보안·시스템 로그를 지운다).
param([Parameter(Mandatory = $true)][string]$Truth)
$ErrorActionPreference = 'Stop'

# 시각 변경 감사(4616)가 남도록 '보안 상태 변경' 감사를 켠다 (언어와 무관한 GUID 사용).
auditpol /set /subcategory:'{0CCE9210-69AE-11D9-BED3-505054503030}' /success:enable | Out-Host

# 로그 삭제 → Security 1102, System 104
wevtutil cl Security
wevtutil cl System
Start-Sleep -Seconds 2

# 사람 계정으로 시계를 20분 앞으로 바꿨다가 되돌린다 → Security 4616
Set-Date -Adjust (New-TimeSpan -Minutes 20) | Out-Null
Start-Sleep -Seconds 2
Set-Date -Adjust (New-TimeSpan -Minutes -20) | Out-Null
w32tm /resync /nowait 2>$null | Out-Null
Start-Sleep -Seconds 3

# 프로그램 실행 기록(Prefetch): 서버판은 기본으로 꺼져 있으므로 켜고, 이름이 겹치지 않는 프로그램을 3번 실행한다.
$pfKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
New-Item -Force $pfKey | Out-Null
Set-ItemProperty $pfKey -Name EnablePrefetcher -Value 3 -Type DWord
try { Set-Service SysMain -StartupType Automatic; Restart-Service SysMain -Force } catch { Write-Host "SysMain: $_" }
Start-Sleep -Seconds 5
New-Item -ItemType Directory -Force C:\st-run | Out-Null
Copy-Item C:\Windows\System32\cmd.exe C:\st-run\STRUN.EXE -Force
$runStart = (Get-Date).ToUniversalTime()
for ($i = 0; $i -lt 3; $i++) {
    Start-Process -FilePath C:\st-run\STRUN.EXE -ArgumentList '/c', 'ping -n 3 127.0.0.1 >nul' -Wait -WindowStyle Hidden
    Start-Sleep -Seconds 12
}
$runEnd = (Get-Date).ToUniversalTime()
Start-Sleep -Seconds 15
$pf = @(Get-ChildItem C:\Windows\Prefetch -Filter 'STRUN.EXE-*.pf' -ErrorAction SilentlyContinue)
Write-Host "Prefetch 파일: $($pf.Count)개"
$pf | Format-Table Name, Length, LastWriteTimeUtc | Out-String | Write-Host

$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$nets = @(Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles' -ErrorAction SilentlyContinue |
    ForEach-Object { (Get-ItemProperty $_.PSPath).ProfileName })
$tzKey = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\TimeZoneInformation').TimeZoneKeyName
@{
    computer_name = $env:COMPUTERNAME
    time_zone     = $tzKey
    build         = $cv.CurrentBuild
    sid           = ([System.Security.Principal.WindowsIdentity]::GetCurrent()).User.Value
    account       = $env:USERNAME
    networks      = $nets
    prefetch      = @{
        exists = ($pf.Count -gt 0)
        name   = 'STRUN.EXE'
        runs   = 3
        start  = $runStart.ToString('yyyy-MM-ddTHH:mm:ssZ')
        end    = $runEnd.ToString('yyyy-MM-ddTHH:mm:ssZ')
    }
} | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8NoBOM $Truth
Get-Content $Truth | Write-Host
