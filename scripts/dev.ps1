# 同时启动 server 和 client
#
# 启动顺序:
#   1. 弹出新窗口跑 server (Tee-Object: 实时日志 + 同步写到 server.log)
#   2. 等 server 端口就绪 (默认 7878, 见 crates/ele_bot_server/src/main.rs 的 bind)
#   3. 原窗口前台跑 client (TUI 占满当前控制台)
#   4. client 退出后, dev.ps1 自动 kill server shell 并清理临时脚本
#
# 用法: powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
# 退出: 关掉 client (TUI 通常按 q) 即自动停 server, 或在 dev.ps1 所在窗口 Ctrl+C

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# 清理旧日志 (client 自己写 client.log)
Remove-Item -Force "$root\server.log", "$root\server.err.log", "$root\client.log" -ErrorAction SilentlyContinue

# 把 server 启动写到临时脚本 — 避 -Command 单行模式下的引号/$ 变量转义
# 主体用 Tee-Object: 输出同时落到新窗口 + server.log, 关窗口前用户能看到最后一行状态
$serverScript = Join-Path $env:TEMP "ele_bot_server_dev_$PID.ps1"
$serverBody = @"
Set-Location -LiteralPath '$root'
cargo run --bin ele_bot_server 2>&1 | Tee-Object -FilePath "$root\server.log"
Write-Host ''
Write-Host 'server exited, press any key to close this window...' -ForegroundColor Yellow
`$null = `$Host.UI.RawUI.ReadKey('NoEcho,IncludeKeyDown')
"@
# UTF-8 无 BOM 写盘, 避免 powershell 把 BOM 当指令输出
[System.IO.File]::WriteAllText($serverScript, $serverBody, (New-Object System.Text.UTF8Encoding $false))

# 弹出新窗口跑 server (用户在这里实时看日志)
$psExe = Join-Path $PSHOME "powershell.exe"
$serverShell = Start-Process `
    -FilePath $psExe `
    -ArgumentList "-NoExit", "-File", $serverScript `
    -WorkingDirectory $root `
    -PassThru

Write-Host "[dev] server launched in a new window (shell pid=$($serverShell.Id))" -ForegroundColor Green
Write-Host "[dev] server log : $root\server.log (Tee-Object mirror of the new window)" -ForegroundColor Green

# 等待 server 监听端口 (默认绑 7878, 见 ele_bot_server/src/main.rs)
# cargo 首次编译可能较久, 给到 90s
$port = 7878
$portReady = $false
for ($i = 0; $i -lt 90; $i++) {
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
    Write-Host "[dev] server didn't open port $port in 90s, see the new window for errors." -ForegroundColor Yellow
}

# 前台跑 client (TUI 占满当前窗口), client 退出时清理 server shell
try {
    cargo run --bin ele_bot_client
} finally {
    if (-not $serverShell.HasExited) {
        Write-Host ""
        Write-Host "[dev] stopping server shell pid=$($serverShell.Id)..." -ForegroundColor Yellow
        Stop-Process -Id $serverShell.Id -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -Force $serverScript -ErrorAction SilentlyContinue
    Write-Host "[dev] done." -ForegroundColor Green
}