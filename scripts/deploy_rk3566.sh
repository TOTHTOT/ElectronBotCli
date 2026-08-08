#!/bin/bash
# 编译并部署到 RK3566 设备
# 用法:
#   ./deploy_rk3566.sh                              - 编译并传输 ele_bot_server + ele_bot_client (release, 默认)
#   ./deploy_rk3566.sh --debug                      - 编译 dev profile (含调试符号, 未优化)
#   ./deploy_rk3566.sh build                        - 只编译两个 (release)
#   ./deploy_rk3566.sh build --debug                - 只编译 (dev)
#   ./deploy_rk3566.sh deploy                        - 只传输
#   ./deploy_rk3566.sh ele_bot_server                - 只编译并传输 ele_bot_server
#   ./deploy_rk3566.sh ele_bot_client                - 只编译并传输 ele_bot_client
#   ./deploy_rk3566.sh test_bd1                      - 编译并传输 test_bd1
#   ./deploy_rk3566.sh test_bd1 --debug              - 编译并传输 test_bd1 (dev)
#
# 可通过环境变量覆盖默认值:
#   RK_DEVICE     目标设备 (默认 radxa@192.168.2.159)
#   RK_REMOTE_DIR 目标路径 (默认 ~/ElectronBotCli)
#   RK_PASSWORD   sshpass 密码 (优先用 ssh 密钥, 仅当密钥失败时回退)
#   HTTP_PROXY    HTTP 代理 (默认 http://192.168.2.147:7890)
#   HTTPS_PROXY   HTTPS 代理 (默认同上)
#   PROFILE       编译 profile, dev / release / release-with-debug (默认 release)

set -euo pipefail

TARGET="aarch64-unknown-linux-gnu"
DEVICE="${RK_DEVICE:-lckfb@192.168.2.248}"
REMOTE_DIR="${RK_REMOTE_DIR:-~/ElectronBotCli}"
HTTP_PROXY="${HTTP_PROXY:-http://192.168.2.147:7890}"
HTTPS_PROXY="${HTTPS_PROXY:-$HTTP_PROXY}"
# 最低可用磁盘空间 (字节), Docker image 缓存 + target/ 至少需要 8GB
MIN_DISK_BYTES=$((8 * 1024 * 1024 * 1024))

# ---------- 工具函数 (前置, 供参数解析使用) ----------

print_help() {
    cat <<'EOF'
deploy_rk3566.sh - 编译并部署到 RK3566 设备

用法:
  ./deploy_rk3566.sh [选项] [模式] [binary]

模式 (可省略, 默认 all = 编译并部署):
  build           只编译
  deploy          只部署
  all             编译并部署 (默认)
  <binary>        直接部署/编译该 binary (等同 all <binary>)

binary (可省略, 默认 ele_bot_server + ele_bot_client 两个都处理):
  ele_bot_server  主程序 (包 ele_bot_server, 部署时带共享库)
  ele_bot_client  客户端程序 (包 ele_bot_client)
  test_bd1        BD1 声音测试程序
  其它任意 [[bin]] 名 (默认归属 ele_bot_server 包)

选项:
  --debug, -d, --dev   使用 dev profile (调试, 含符号未优化)
  --help, -h           显示本帮助并退出

profile (--debug 优先, 否则读 PROFILE 环境变量, 否则 release):
  dev / debug
  release
  release-with-debug   含调试符号的 release

环境变量:
  RK_DEVICE         目标 SSH 地址         (默认 radxa@192.168.2.159)
  RK_REMOTE_DIR     远程部署目录          (默认 ~/ElectronBotCli)
  RK_PASSWORD       sshpass 密码          (默认 radxa, 优先用 SSH 密钥)
  HTTP_PROXY        HTTP 代理             (默认 http://192.168.2.147:7890)
  HTTPS_PROXY       HTTPS 代理            (默认同上)
  PROFILE           cargo profile 名      (默认 release)

示例:
  ./deploy_rk3566.sh                                    # 编译并部署 ele_bot_server (release)
  ./deploy_rk3566.sh build                              # 只编译
  ./deploy_rk3566.sh deploy                             # 只部署已编译产物
  ./deploy_rk3566.sh --debug                            # debug 编译
  ./deploy_rk3566.sh test_bd1                           # 编译并部署 test_bd1
  PROFILE=release-with-debug ./deploy_rk3566.sh         # 自定义 profile
  RK_DEVICE=radxa@192.168.2.202 ./deploy_rk3566.sh      # 换目标设备
  ssh-copy-id radxa@192.168.2.159                       # 推荐: 先做密钥免密

内置检查:
  docker daemon / cross 二进制 / 磁盘 ≥8GB
  传输前 sha256 比对, 设备端已是同一文件则跳过 (不重启进程)
  scp 后 sha256 校验 (传输截断自动报错)
  cargo 缓存挂载进 cross 容器 (增量编译命中)
EOF
}

# 解析参数: 提取 --debug 标志, 其他位置参数
PROFILE_FLAG=""
POSITIONAL=()
for arg in "$@"; do
    case "$arg" in
        --help|-h)
            print_help
            exit 0
            ;;
        --debug|--dev|-d)
            PROFILE_FLAG="--debug"
            ;;
        -*)
            echo "未知选项: $arg" >&2
            print_help >&2
            exit 1
            ;;
        *)
            POSITIONAL+=("$arg")
            ;;
    esac
done

# 解析 mode 和 binary: 未指定 binary 时默认两个主程序都处理
MODE="${POSITIONAL[0]:-all}"
case "$MODE" in
    build|deploy|all)
        BINARY="${POSITIONAL[1]:-}"
        ;;
    *)
        # 单参数直接是 binary 名 (e.g. ./deploy_rk3566.sh test_bd1)
        BINARY="$MODE"
        MODE="all"
        ;;
esac

if [[ -n "$BINARY" ]]; then
    BINARIES=("$BINARY")
else
    BINARIES=("ele_bot_server" "ele_bot_client")
fi

# binary 所属 crate: ele_bot_client 独立成包, 其余都属 ele_bot_server
package_of() {
    if [[ "$1" == "ele_bot_client" ]]; then
        echo "ele_bot_client"
    else
        echo "ele_bot_server"
    fi
}

# 解析 profile: 优先 --debug 标志, 再 PROFILE 环境变量
if [[ -n "$PROFILE_FLAG" ]]; then
    PROFILE="dev"
elif [[ -n "${PROFILE:-}" ]]; then
    PROFILE="$PROFILE"
else
    PROFILE="release"
fi

# 路径段: dev -> debug, 其余用 profile 原名 (release / release-with-debug)
case "$PROFILE" in
    dev|debug) PROFILE_DIR="debug" ;;
    *)         PROFILE_DIR="$PROFILE" ;;
esac

binary_path() {
    echo "target/$TARGET/$PROFILE_DIR/$1"
}

# 传给 cross 的 flag: dev -> 无 --release, release -> --release, 其他 -> --profile <name>
# 先初始化, 配合 set -u 避免空数组解引用
CROSS_PROFILE_FLAG=""
case "$PROFILE" in
    dev|debug)
        ;;
    release)
        CROSS_PROFILE_FLAG="--release"
        ;;
    *)
        CROSS_PROFILE_FLAG="--profile $PROFILE"
        ;;
esac

# ---------- 前置检查 ----------

check_docker() {
    if ! command -v docker >/dev/null 2>&1; then
        echo "错误: 未安装 docker, 请先安装 Docker Desktop 或 docker-cli"
        exit 1
    fi
    if ! docker info >/dev/null 2>&1; then
        echo "错误: docker daemon 未运行, 请启动 Docker Desktop"
        echo "      (macOS: 菜单栏图标 → Restart)"
        exit 1
    fi
}

check_disk() {
    # 拿当前目录所在文件系统的可用空间
    local avail
    avail=$(df -k . | tail -1 | awk '{print $4}')
    local avail_bytes=$((avail * 1024))
    if (( avail_bytes < MIN_DISK_BYTES )); then
        echo "警告: 可用磁盘仅 $((avail_bytes / 1024 / 1024 / 1024))GB, 低于推荐 $((MIN_DISK_BYTES / 1024 / 1024 / 1024))GB"
        echo "      编译 + Docker 缓存可能耗尽磁盘"
        echo "      建议: docker builder prune 或清理 ./target"
        read -rp "继续? [y/N] " ans
        [[ "$ans" == "y" || "$ans" == "Y" ]] || exit 1
    fi
}

check_cross() {
    if ! command -v cross >/dev/null 2>&1; then
        echo "错误: 未安装 cross"
        echo "      安装: cargo install cross --git https://github.com/cross-rs/cross"
        exit 1
    fi
}

check_binary_exists() {
    if [[ ! -x "$1" ]]; then
        echo "错误: $1 不存在或不可执行, 请先 build"
        exit 1
    fi
}

# ---------- SSH 传输 ----------

# 优先用 ssh 密钥 (无密码), 失败则用 sshpass (有密码时)
# sshpass 仅在 RK_PASSWORD 设置或交互式询问后使用
deploy_binary() {
    local local_path="$1"
    local remote_path="$2"

    # 计算本地 sha256, 先与设备端比对
    local local_hash
    local_hash=$(shasum -a 256 "$local_path" | awk '{print $1}')
    echo "本地 sha256: $local_hash"

    # 设备端已有同一文件则跳过传输 (不 pkill: 文件没变, 不动正在跑的进程;
    # .so / zeroclaw 这类基本不变的文件每次部署能省下大头传输时间).
    # 连接失败时静默继续走正常传输路径, 由 scp/校验阶段报错
    local remote_hash
    remote_hash=$(run_remote_cmd "sha256sum $remote_path 2>/dev/null | awk '{print \$1}'" 2>/dev/null || true)
    if [[ -n "$remote_hash" && "$remote_hash" == "$local_hash" ]]; then
        echo "设备端已是同一文件 (sha256 一致), 跳过传输"
        return 0
    fi

    # 先杀掉设备上正在运行的同名进程, 否则 scp 会因 "text file busy"
    # (dest open Failure) 传不上去
    local bin_name
    bin_name=$(basename "$remote_path")
    echo "停止设备上运行中的 $bin_name ..."
    run_remote_cmd "pkill -x '$bin_name' 2>/dev/null; pkill -f '^\\./$bin_name' 2>/dev/null; sleep 1; true" || true

    # 先尝试 ssh 密钥
    if ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 \
        "$DEVICE" true 2>/dev/null; then
        echo "使用 SSH 密钥传输..."
        scp -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 \
            "$local_path" "$DEVICE:$remote_path"
    else
        # 密钥失败, 回退 sshpass
        if ! command -v sshpass >/dev/null 2>&1; then
            echo "错误: SSH 密钥登录失败, 且未安装 sshpass"
            echo "      选项: 1) ssh-copy-id $DEVICE  2) brew install sshpass  3) 设置 RK_PASSWORD"
            exit 1
        fi
        local pass="${RK_PASSWORD:-radxa}"
        echo "使用 sshpass 传输 (密码来自 RK_PASSWORD 或默认值)..."
        sshpass -p "$pass" scp -o StrictHostKeyChecking=accept-new \
            "$local_path" "$DEVICE:$remote_path"
    fi

    # 设备端校验
    echo "设备端校验 sha256..."
    local remote_hash
    remote_hash=$(run_remote_cmd "sha256sum $remote_path 2>/dev/null | awk '{print \$1}'") || {
        echo "错误: 无法连接设备执行校验, 请检查 SSH 认证或 RK_PASSWORD"
        exit 1
    }

    if [[ -z "$remote_hash" ]]; then
        echo "错误: 设备端未找到文件或 sha256sum 返回为空"
        echo "  远程路径: $remote_path"
        exit 1
    fi

    if [[ "$remote_hash" != "$local_hash" ]]; then
        echo "错误: sha256 不一致!"
        echo "  本地: $local_hash"
        echo "  设备: $remote_hash"
        echo "传输可能被截断, 请重试"
        exit 1
    fi
    echo "sha256 校验通过"
}

# 执行远程命令, 复用与 deploy_binary 相同的认证回退逻辑.
# 优先使用 SSH 密钥 (BatchMode); 密钥失败时回退到 sshpass.
run_remote_cmd() {
    local cmd="$1"
    # 先尝试 SSH 密钥
    if ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 \
        "$DEVICE" "$cmd" 2>/dev/null; then
        return 0
    fi
    # 密钥失败, 回退 sshpass
    if ! command -v sshpass >/dev/null 2>&1; then
        echo "错误: SSH 密钥登录失败, 且未安装 sshpass" >&2
        echo "      选项: 1) ssh-copy-id $DEVICE  2) brew install sshpass  3) 设置 RK_PASSWORD" >&2
        return 1
    fi
    local pass="${RK_PASSWORD:-radxa}"
    if ! sshpass -p "$pass" ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 \
        "$DEVICE" "$cmd"; then
        echo "错误: 通过 sshpass 执行远程命令失败" >&2
        return 1
    fi
}

# ---------- build / deploy ----------

build_binary() {
    check_docker
    check_cross
    check_disk
    local bin pkg
    for bin in "${BINARIES[@]}"; do
        pkg=$(package_of "$bin")
        echo "=== 编译 $bin (包 $pkg, profile: $PROFILE) ==="
        if ! HTTP_PROXY="$HTTP_PROXY" HTTPS_PROXY="$HTTPS_PROXY" \
             cross build $CROSS_PROFILE_FLAG --target "$TARGET" -p "$pkg" --bin "$bin"; then
            echo "编译失败！"
            exit 1
        fi
    done
    echo "=== 编译完成 ==="
}

deploy_step() {
    echo "=== 传输到设备 ==="
    local bin pkg so
    for bin in "${BINARIES[@]}"; do
        pkg=$(package_of "$bin")
        check_binary_exists "$(binary_path "$bin")"
        deploy_binary "$(binary_path "$bin")" "$REMOTE_DIR/$bin"
        # sherpa-onnx-sys 会把依赖的共享库拷到产物旁 (libonnxruntime /
        # libsherpa-onnx-c-api / libsherpa-onnx-cxx-api), 二进制靠 $ORIGIN
        # rpath 在同目录找它们, 必须一起上传 (仅 server 包需要)
        if [[ "$pkg" == "ele_bot_server" ]]; then
            for so in "target/$TARGET/$PROFILE_DIR"/*.so; do
                [[ -e "$so" ]] || continue
                deploy_binary "$so" "$REMOTE_DIR/$(basename "$so")"
            done
            # librknnrt 也随二进制走 $ORIGIN: 设备系统自带的版本 (2.3.x)
            # 与 sherpa-onnx rknn 模型不兼容, 固定用 assets/lib 里验证过的版本
            deploy_binary "assets/lib/librknnrt.so" "$REMOTE_DIR/librknnrt.so"
            # librga 同理: 设备系统自带的是 rga_api 1.3.2 (YUYV CSC 输出全绿),
            # 必须用 assets/lib 里的官方 1.10.6 预编译版
            deploy_binary "assets/lib/librga.so" "$REMOTE_DIR/librga.so"
            # zeroclaw: LLM 对话/记忆托管进程 (aarch64 musl 静态版, 锁定 v0.8.3)
            # 只下发二进制; zeroclaw 配置 (provider/api_key/人设) 由用户在
            # 设备上自行维护 (默认 ~/.zeroclaw), 不随部署覆盖
            deploy_binary "assets/zeroclaw/zeroclaw" "$REMOTE_DIR/zeroclaw"
        fi
    done
    echo "=== 传输完成 ==="
}

case "$MODE" in
    build)
        build_binary
        ;;
    deploy)
        deploy_step
        ;;
    all|"")
        build_binary
        deploy_step
        ;;
    *)
        echo "未知模式: $MODE" >&2
        print_help >&2
        exit 1
        ;;
esac

echo ""
echo "在设备上运行："
for bin in "${BINARIES[@]}"; do
    echo "  ./$bin"
done
if [[ " ${BINARIES[*]} " == *" ele_bot_server "* ]]; then
    echo "  测试人脸检测: TEST_RKNN=1 ./ele_bot_server"
    echo "  测试指定模型: TEST_RKNN=1 RKNN_MODEL=./model/deepghs/yolo-face/yolo_face.rknn ./ele_bot_server"
    echo "  测试指定图片: TEST_RKNN=1 TEST_IMAGE=./assets/images/test.png ./ele_bot_server"
fi