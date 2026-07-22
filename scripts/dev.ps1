# 同时启动 server 和 client
#
# 启动顺序:
#   1. 后台启动 server, 日志重定向到 server.log / server.err.log (不抢控制台)
#   2. 等 server 端口就绪 (默认 7878, 见 crates/ele_bot_server/src/main.rs 的 bind)
#   3. 打开新窗口启动 client (TUI, 不阻塞 dev.ps1 所在窗口)
#   4. 关掉 client 窗口后, dev.ps1 自动 kill server 并清理临时脚本
#
# 用法: powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
# 退出: 关闭 client 窗口 (推荐), 或在 dev.ps1 所在窗口 Ctrl+C 强制停 server

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# 清理旧日志 (server 写 server.log / server.err.log, client 自己写 client.log)
Remove-Item -Force "$root\server.log", "$root\server.err.log", "$root\client.log" -ErrorAction SilentlyContinue

# 后台启动 server, 重定向到日志文件, 不开新窗口 — 不抢控制台, server 的实时输出走文件
$serverProc = Start-Process `
    -FilePath "cargo" `
    -ArgumentList "run", "--bin", "ele_bot_server" `
    -WorkingDirectory $root `
    -RedirectStandardOutput "$root\server.log" `
    -RedirectStandardError "$root\server.err.log" `
    -PassThru `
    -NoNewWindow

Write-Host "[dev] server started, pid=$($serverProc.Id)" -ForegroundColor Green
Write-Host "[dev] server log : $root\server.log" -ForegroundColor Green

# 等待 server 监听端口 (默认绑 7878, 见 ele_bot_server/src/main.rs)
$port = 7878
$portReady = $false
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Seconds 1
    $tcp = New-Object System.Net.Sockets.TcpClient
    try {
        $iar = $tcp.BeginConnect("127.0.0.1", $port, $null, $null)
        if ($iar.AsyncWaitHandle.WaitOne(200) -and $tcp.Connected) {
            $portReady = $true
        }
    } catch {
        # 连接失败 — server 还没起来, 继续等
    }
    $tcp.Close()
    if ($portReady) { break }
}

if (-not $portReady) {
    Write-Host "[dev] server didn't open port $port in 30s, see $root\server.log" -ForegroundColor Yellow
}

# 把 client 启动逻辑写到临时脚本 — 避开 -Command 单行模式下的引号/$ 变量转义
$clientScript = Join-Path $env:TEMP "ele_bot_client_dev_$PID.ps1"
$clientBody = @"
Set-Location -LiteralPath '$root'
cargo run --bin ele_bot_client
`$code = `$LASTEXITCODE
Write-Host ''
Write-Host "client exited (code=`$code), closing window in 2s..." -ForegroundColor Yellow
Start-Sleep -Seconds 2
"@
# 用 UTF-8 无 BOM 写盘, 避免 powershell 把 BOM 当指令输出
[System.IO.File]::WriteAllText($clientScript, $clientBody, (New-Object System.Text.UTF8Encoding $false))

# 用与当前一致的 powershell 启动新窗口跑 client
$psExe = Join-Path $PSHOME "powershell.exe"
$clientShell = Start-Process `
    -FilePath $psExe `
    -ArgumentList "-File", $clientScript `
    -WorkingDirectory $root `
    -PassThru

Write-Host "[dev] client launched in new window (shell pid=$($clientShell.Id))" -ForegroundColor Green
Write-Host "[dev] tail server log:  Get-Content -Wait '$root\server.log'" -ForegroundColor Green
Write-Host "[dev] close the client window to stop server (or Ctrl+C here to stop server only)." -ForegroundColor Yellow

# 阻塞, 等 client 窗口关闭 (powershell 进程退出) 后清理 server
try {
    Wait-Process -Id $clientShell.Id -ErrorAction SilentlyContinue
    Write-Host "[dev] client window closed." -ForegroundColor Green
} finally {
    Remove-Item -Force $clientScript -ErrorAction SilentlyContinue
    if (-not $serverProc.HasExited) {
        Write-Host "[dev] stopping server pid=$($serverProc.Id)..." -ForegroundColor Yellow
        Stop-Process -Id $serverProc.Id -Force -ErrorAction SilentlyContinue
    }
    Write-Host "[dev] done." -ForegroundColor Green
}
