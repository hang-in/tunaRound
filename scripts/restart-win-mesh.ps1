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

# 브로커 토큰 가드: 이 스크립트는 브로커를 `serve 0.0.0.0:8770`(비-loopback)으로 띄우므로 토큰이
# 없거나 init 스캐폴드의 placeholder 그대로면 /mcp·/a2a·대시보드 쓰기가 LAN에 무인증으로 열린다.
# 데몬들도 같은 env를 상속해 mesh가 정상 동작하는 탓에 어떤 실패 신호도 없다(fail-visible). 여기서 막는다.
# 공백만 있는 토큰(" ")이나 앞뒤 공백이 붙은 placeholder(" 여기에-...-넣기 ")도 무효로 본다(coderabbit
# Major): trim 후 빈 문자열·placeholder면 거부.
$BrokerTokenTrimmed = if ($env:TUNA_BROKER_TOKEN) { $env:TUNA_BROKER_TOKEN.Trim() } else { '' }
if (-not $BrokerTokenTrimmed -or $BrokerTokenTrimmed -eq '여기에-실제-토큰-넣기') {
    throw "config에 TUNA_BROKER_TOKEN이 없거나 공백/placeholder입니다: $ConfigPath (무토큰/placeholder면 0.0.0.0 브로커가 무인증으로 뜬다. init 안내대로 실제 토큰을 채우세요)"
}

# 1b. -SourceBin 존재 검증(#1 확정 방침): 기존 데몬을 죽이는 step 2보다 먼저 검증한다. 여기서 throw해도
#     아직 아무 데몬도 안 죽인 상태라 "기동된 브로커 PID가 미기록 고아로 남는" 실패 창이 없다.
if ($SourceBin -and -not (Test-Path $SourceBin)) { throw "SourceBin 없음: $SourceBin" }

# 2. 기존 mesh 데몬 종료: 이전 실행이 기록한 mesh.pids의 PID만(tunaround 이름 검증 후) 죽인다.
#    tunaround.exe 전수 종료는 다른 세션들의 수신 poll(Monitor)까지 죽이던 실측(luckyCAD
#    "수십 분 후 exit 127" x3)이 있어 폐기. mesh.pids가 없으면(최초/마이그레이션) 포트 소유자를
#    알 수 없어 한 번만 전수 종료로 폴백한다(그 1회는 세션 poll 재무장 필요).
#    codex.exe는 죽이지 않는다(운영자의 보이는 codex 세션일 수 있음). app-server는 포트로 생존 판정.
#    재부팅 후에는 mesh.pids가 stale이고 Windows가 그 PID 번호를 새 프로세스(방금 뜬 세션 poll 등)에
#    재할당할 수 있어, 이름 검증만으론 구별 못 하고 살아있는 세션 poll을 죽일 수 있다(#2 확정 방침).
#    부팅 시각이 mesh.pids 기록 시각보다 나중이면(=그 사이 재부팅) 파일을 통째로 stale로 보고 종료
#    자체를 건너뛴다 - 재부팅이 이전 mesh를 이미 다 죽였으므로 정리할 옛 데몬이 없다(전수 폴백도 안 함).
$PidsFile = Join-Path $TunaHome "mesh.pids"
$rebootedSincePids = $false
if (Test-Path $PidsFile) {
    try {
        $lastBoot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime
        $pidsWritten = (Get-Item $PidsFile).LastWriteTime
        if ($lastBoot -gt $pidsWritten) { $rebootedSincePids = $true }
    } catch {
        # 부팅 시각 조회 실패 시 판단 신호가 없으므로 기존 선별 종료 경로로 폴백한다(보수적).
    }
}
if ($rebootedSincePids) {
    Write-Host "[mesh] 재부팅 감지(부팅 $lastBoot > mesh.pids 기록 $pidsWritten) - mesh.pids stale, 종료 스킵(세션 poll 오살 방지)"
    Remove-Item $PidsFile -Force -ErrorAction SilentlyContinue
} elseif (Test-Path $PidsFile) {
    foreach ($meshPid in (Get-Content $PidsFile | Where-Object { $_ -match '^\d+$' })) {
        $p = Get-Process -Id ([int]$meshPid) -ErrorAction SilentlyContinue
        if ($p -and $p.ProcessName -eq 'tunaround') {
            Write-Host "[mesh] 데몬 종료 PID=$meshPid"
            # 체크와 종료 사이 프로세스가 스스로 죽는 레이스가 전체 재기동을 중단시키지 않게
            # ($ErrorActionPreference=Stop 전역이라 미지정 시 종단 오류 승격, 봇리뷰 Major).
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Remove-Item $PidsFile -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
} else {
    $procs = Get-Process tunaround -ErrorAction SilentlyContinue
    if ($procs) {
        Write-Host "[mesh] mesh.pids 없음 - 전수 종료 폴백($($procs.Count)개, 세션 수신 poll은 재무장 필요)"
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }
}

# 3. (옵션) 새 바이너리 배포: rename-swap. 실행 중 exe도 rename은 되므로, 살아있는 세션 poll은
#    옛 이미지(rename된 파일)로 계속 돌고 다음 재무장 때 새 빌드를 탄다(재배포 != 세션 수신 단절).
if ($SourceBin) {
    # 존재 검증은 위 1b에서 이미 끝냈다(step 2 kill 전에 검증 = #1 확정 방침).
    New-Item -ItemType Directory -Force $StableDir | Out-Null
    # 이전 스왑 잔재 정리(아직 물고 있는 프로세스가 있으면 삭제가 실패해도 무해 - 다음 실행이 재시도).
    Get-ChildItem $StableDir -Filter "tunaround-old-*.exe" -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue }
    # 복사를 먼저 .new로 성사시킨 뒤 스왑한다 - rename 후 복사 실패 시 안정 바이너리가 통째로
    # 사라지는 창 제거(봇리뷰 Major, mac 원자 재배포 cp .new→mv와 같은 규약). Rename-Item의
    # NewName은 경로가 아니라 이름(leaf)만 준다(봇리뷰 high).
    $NewBin = Join-Path $StableDir "tunaround.exe.new"
    Copy-Item $SourceBin $NewBin -Force
    if (Test-Path $StableBin) {
        Rename-Item $StableBin ("tunaround-old-{0}.exe" -f (Get-Date -Format "yyyyMMddHHmmss"))
    }
    Rename-Item $NewBin "tunaround.exe"
    Write-Host "[mesh] 바이너리 배포(rename-swap): $SourceBin -> $StableBin"
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

# PID를 mesh.pids에 기동 직후 즉시 append한다(#1 확정 방침): 모든 데몬이 뜬 뒤 일괄 기록하면, 중간
# 단계의 throw(app-server 실패 등)로 재기동이 중단될 때 이미 뜬 데몬이 mesh.pids에 전혀 안 남아
# 고아 PID가 된다. 기동마다 바로 기록해 어느 시점에 중단돼도 그때까지 뜬 데몬은 항상 추적된다.
function Start-DaemonTracked([string]$Name, [string]$Exe, [string[]]$ArgList) {
    $p = Start-Daemon $Name $Exe $ArgList
    Add-Content -Path $PidsFile -Value $p.Id
    return $p.Id
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
#    이 스크립트가 띄운 데몬 PID는 기동 직후 즉시 mesh.pids에 append된다(다음 실행의 선별 종료 근거).
$meshPids = @()
$meshPids += Start-DaemonTracked "broker" $StableBin @("serve", "0.0.0.0:8770", "--db", $BrokerDb)
$ok = $false
foreach ($i in 1..30) {
    if (Test-Port 8770) { $ok = $true; break }
    Start-Sleep -Seconds 1
}
if (-not $ok) { throw "브로커가 30초 내 8770 listen 안 함. ~/.tunaround/broker.err.log 확인" }
Write-Host "[mesh] 브로커 8770 listen 확인"

# 5. codex app-server(8790): 브로커 이후에 떠야 tuna-broker MCP 로드 성공(세션18 실측). 살아있으면 유지.
#    기동했으면 listen까지 대기 - relay가 준비 전 주입을 시도해 fail_task로 새지 않게(봇리뷰 Major).
if (Test-Port 8790) {
    Write-Host "[mesh] codex app-server 8790 이미 listen(유지)"
} elseif (Test-Path $CodexExe) {
    Start-Daemon "codex-appserver" $CodexExe @("app-server", "--listen", "ws://127.0.0.1:8790") | Out-Null
    $appOk = $false
    foreach ($i in 1..30) {
        if (Test-Port 8790) { $appOk = $true; break }
        Start-Sleep -Seconds 1
    }
    if ($appOk) { Write-Host "[mesh] codex app-server 8790 listen 확인" }
    else { Write-Host "[mesh] 경고: app-server가 30초 내 8790 listen 안 함(relay 주입은 실패 시 fail_task)" }
} else {
    Write-Host "[mesh] codex.exe 없음($CodexExe) - app-server 생략"
}

# 6. presence 스캐너(머신당 1, v2-44): core/token/machine은 config env 폴백.
$meshPids += Start-DaemonTracked "presence-scan" $StableBin @("presence-scan")

# 7. codex 배달 데몬(v2-46, 구 codex-sup poll+핸들러 대체): 로컬 codex 세션들 앞 task를
#    대리 claim해 그 세션 thread로 in-process 주입. core/token/machine은 config env 폴백.
$meshPids += Start-DaemonTracked "codex-relay" $StableBin @("codex-relay", "--ws", "ws://127.0.0.1:8790")

# 8. 총괄 결과 인박스(watch-results, digest 60초).
$meshPids += Start-DaemonTracked "watch-results" $StableBin @("watch-results", "--core", $BaseUrl, "--dispatcher", "dashboard", "--digest", "60")

# 9. PID 기록은 각 기동 직후 Start-DaemonTracked가 이미 mesh.pids에 append했다(#1 확정 방침).
#    app-server(codex.exe)는 종료 대상이 아니라 기록하지 않는다. 여기서 별도 일괄 기록은 하지 않는다
#    (append만으로 충분 - 중복 기록 방지).

Start-Sleep -Seconds 3
Write-Host "[mesh] 완료. 상태:"
Write-Host ("  8770(broker): " + (Test-Port 8770))
Write-Host ("  8790(app-server): " + (Test-Port 8790))
Write-Host ("  mesh 데몬 PID: " + ($meshPids -join ", ") + " (mesh.pids 기록)")
Write-Host "[mesh] 세션 수신 poll은 건드리지 않음(선별 종료). 전수 폴백이 떴던 경우에만 재무장 필요."
