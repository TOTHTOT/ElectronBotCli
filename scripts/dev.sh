#!/usr/bin/env bash
# 同时启动 server 和 client
# 用法: ./scripts/dev.sh
# 退出: Ctrl+C, 残余进程会由 trap 清理

set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT=7879

rm -f "$ROOT/server.log" "$ROOT/client.log"

cleanup() {
    echo ""
    echo "stopping server pid=$SERVER_PID..."
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# 后台启动 server
cargo run --bin ele_bot_server > "$ROOT/server.log" 2>&1 &
SERVER_PID=$!
echo "server started, pid=$SERVER_PID"

# 等待 server 监听端口
for i in $(seq 1 30); do
    sleep 1
    if (echo > /dev/tcp/127.0.0.1/$PORT) 2>/dev/null; then
        break
    fi
done

# 前台启动 client
cargo run --bin ele_bot_client
