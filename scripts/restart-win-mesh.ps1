# Windows mesh 멱등 재기동 스크립트(restart-mac-mesh.sh의 윈도우 대응물, 재부팅 복구 = 이 파일 한 줄).
# 사용법:
#   pwsh -File scripts\restart-win-mesh.ps1                      # 재기동만(재부팅 후 복구)
#   pwsh -File scripts\restart-win-mesh.ps1 -SourceBin <exe경로>  # 새 빌드를 안정 경로로 배포 후 재기동
# 기동 순서(실측 제약): 브로커 listen 확인 -> codex app-server -> presence-scan / codex-sup poll / watch-results.
# 토큰·코어 URL은 ~/.tunaround/config에서 읽어 자식에 env로 상속(argv 비노출, 레포에 시크릿 없음).
param(
    [string]$SourceBin = ""
)

$ErrorActionPreference = "Stop"

$TunaHome = Join-Path $HOME ".tunaround"
$StableDir = Join-Path $env:LOCALAPPDATA "tunaround\bin"
$StableBin = Join-Path $StableDir "tunaround.exe"
$BrokerDb = Join-Path $env:LOCALAPPDATA "tunaround\broker.db"
$CodexExe = Join-Path $env:APPDATA "npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe"

# 1. config 로드(config-first, v2-43 §5-1): 자식 프로세스가 env 폴백으로 읽는다.
$ConfigPath = Join-Path $TunaHome "config"
if (-not (Test-Path $ConfigPath)) { throw "config 없음: $ConfigPath" }
Get-Content $ConfigPath | Where-Object { $_ -match '^\s*TUNA_\w+\s*=' } | ForEach-Object {
    $k, $v = $_ -split '=', 2
    # 값에 따옴표가 있으면 자식 프로세스의 경로·URL 파싱이 깨지므로 벗긴다(봇리뷰 medium).
    Set-Item -Path "env:$($k.Trim())" -Value $v.Trim().Trim("'").Trim('"')
}
$Core = $env:TUNA_BROKER_CORE  # 예: http://127.0.0.1:8770/mcp
if (-not $Core) { throw "config에 TUNA_BROKER_CORE 없음: $ConfigPath (빈 --core로 데몬이 조용히 죽는다)" }
$BaseUrl = $Core -replace '/mcp/?$', ''

# 2. 기존 tunaround 프로세스 전부 종료.
#    주의: 세션 수신 poll(Monitor)도 같이 죽는다. 살아있는 감독 세션은 재기동 후 poll을 다시 무장해야 한다.
#    codex.exe는 죽이지 않는다(운영자의 보이는 codex 세션일 수 있음). app-server는 포트로 생존 판정.
$procs = Get-Process tunaround -ErrorAction SilentlyContinue
if ($procs) {
    Write-Host "[mesh] tunaround 프로세스 $($procs.Count)개 종료"
    $procs | Stop-Process -Force
    Start-Sleep -Seconds 2
}

# 3. (옵션) 새 바이너리 배포: 안정 경로 스왑(재빌드 != mesh 중단 원칙, 배포는 이 스크립트 경유만).
if ($SourceBin) {
    if (-not (Test-Path $SourceBin)) { throw "SourceBin 없음: $SourceBin" }
    New-Item -ItemType Directory -Force $StableDir | Out-Null
    Copy-Item $SourceBin $StableBin -Force
    Write-Host "[mesh] 바이너리 배포: $SourceBin -> $StableBin"
}
if (-not (Test-Path $StableBin)) { throw "안정 바이너리 없음: $StableBin (최초엔 -SourceBin으로 배포)" }

# 주의: 둘째 인자 이름을 $Args로 지으면 PowerShell 자동 변수와 충돌해 인자가 증발한다(실측: serve가 REPL로 폴백).
# Start-Process -ArgumentList는 배열을 인용 없이 공백 join하므로(버전 불문 고질), 공백 인자는 직접 인용한다(봇리뷰 high).
function Start-Daemon([string]$Name, [string]$Exe, [string[]]$ArgList) {
    $out = Join-Path $TunaHome "$Name.log"
    $err = Join-Path $TunaHome "$Name.err.log"
    $argString = ($ArgList | ForEach-Object { if ($_ -match '\s') { '"{0}"' -f $_ } else { $_ } }) -join ' '
    $p = Start-Process -FilePath $Exe -ArgumentList $argString -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $out -RedirectStandardError $err
    Write-Host "[mesh] $Name 기동 PID=$($p.Id)"
    return $p
}

# Get-NetTCPConnection(CIM)은 이 머신에서 부하 시 행에 걸린 실측이 있어 TcpClient 직결로 판정한다.
function Test-Port([int]$Port) {
    $c = New-Object System.Net.Sockets.TcpClient
    try {
        if ($c.ConnectAsync("127.0.0.1", $Port).Wait(1000)) { return $true }
        return $false
    } catch { return $false } finally { $c.Dispose() }
}

# 4. 브로커 기동 + listen 대기(watcher 레이스 회피, 세션13 실측).
Start-Daemon "broker" $StableBin @("serve", "0.0.0.0:8770", "--db", $BrokerDb) | Out-Null
$ok = $false
foreach ($i in 1..30) {
    if (Test-Port 8770) { $ok = $true; break }
    Start-Sleep -Seconds 1
}
if (-not $ok) { throw "브로커가 30초 내 8770 listen 안 함. ~/.tunaround/broker.err.log 확인" }
Write-Host "[mesh] 브로커 8770 listen 확인"

# 5. codex app-server(8790): 브로커 이후에 떠야 tuna-broker MCP 로드 성공(세션18 실측). 살아있으면 유지.
if (Test-Port 8790) {
    Write-Host "[mesh] codex app-server 8790 이미 listen(유지)"
} elseif (Test-Path $CodexExe) {
    Start-Daemon "codex-appserver" $CodexExe @("app-server", "--listen", "ws://127.0.0.1:8790") | Out-Null
} else {
    Write-Host "[mesh] codex.exe 없음($CodexExe) - app-server 생략"
}

# 6. presence 스캐너(머신당 1, v2-44): core/token/machine은 config env 폴백.
Start-Daemon "presence-scan" $StableBin @("presence-scan") | Out-Null

# 7. win-codex-sup 감독 poll(infra, codex-inject 글루 핸들러).
$Handler = Join-Path $TunaHome "codex-sup-handle.cmd"
Start-Daemon "codex-sup" $StableBin @("poll", "--core", $Core, "--agent", "win-codex-sup", "--tags", "machine=win,purpose=codex-inject,role=infra,runner=codex", "--on-task", $Handler) | Out-Null

# 8. 총괄 결과 인박스(watch-results, digest 60초).
Start-Daemon "watch-results" $StableBin @("watch-results", "--core", $BaseUrl, "--dispatcher", "dashboard", "--digest", "60") | Out-Null

Start-Sleep -Seconds 3
Write-Host "[mesh] 완료. 상태:"
Write-Host ("  8770(broker): " + (Test-Port 8770))
Write-Host ("  8790(app-server): " + (Test-Port 8790))
Get-Process tunaround -ErrorAction SilentlyContinue | ForEach-Object { Write-Host ("  tunaround PID=" + $_.Id) }
Write-Host "[mesh] 감독 세션 수신 poll은 각 세션에서 재무장 필요(훅이 새 세션엔 자동 주입)."
