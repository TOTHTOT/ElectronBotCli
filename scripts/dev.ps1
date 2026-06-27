# 同时启动 server 和 client
# 用法: powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
# 退出: 按 Ctrl+C, 然后运行 scripts/stop.ps1 结束残余进程

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# 清理旧日志
Remove-Item -Force "$root\server.log", "$root\client.log" -ErrorAction SilentlyContinue

# 后台启动 server
$serverProc = Start-Process `
    -FilePath "cargo" `
    -ArgumentList "run", "--bin", "ele_bot_server" `
    -WorkingDirectory $root `
    -RedirectStandardOutput "$root\server.log" `
    -RedirectStandardError "$root\server.err.log" `
    -PassThru `
    -NoNewWindow

Write-Host "server started, pid=$($serverProc.Id)"

# 等待 server 启动 (监听端口)
$portReady = $false
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Seconds 1
    try {
        $client = New-Object System.Net.Sockets.TcpClient
        $client.BeginConnect("127.0.0.1", 7879, $null, $null) | Out-Null
        Start-Sleep -Milliseconds 200
        if ($client.Connected) {
            $portReady = $true
            $client.Close()
            break
        }
    } catch {
        $client.Close()
    }
}

if (-not $portReady) {
    Write-Host "server didn't open port 7879 in 30s, see server.log" -ForegroundColor Yellow
}

# 前台启动 client (Ctrl+C 即退出)
try {
    cargo run --bin ele_bot_client
} finally {
    Write-Host "client exited, killing server pid=$($serverProc.Id)..."
    Stop-Process -Id $serverProc.Id -Force -ErrorAction SilentlyContinue
}
