#!/bin/bash
# 编译并部署到 RK3566 设备
# 用法:
#   ./deploy_rk3566.sh                              - 编译并传输 ele_bot_server (release, 默认)
#   ./deploy_rk3566.sh --debug                      - 编译 dev profile (含调试符号, 未优化)
#   ./deploy_rk3566.sh build                        - 只编译 (release)
#   ./deploy_rk3566.sh build --debug                - 只编译 (dev)
#   ./deploy_rk3566.sh deploy                        - 只传输
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
DEVICE="${RK_DEVICE:-radxa@192.168.2.159}"
REMOTE_DIR="${RK_REMOTE_DIR:-~/ElectronBotCli}"
HTTP_PROXY="${HTTP_PROXY:-http://192.168.2.147:7890}"
HTTPS_PROXY="${HTTPS_PROXY:-$HTTP_PROXY}"
# 最低可用磁盘空间 (字节), Docker image 缓存 + target/ 至少需要 8GB
MIN_DISK_BYTES=$((8 * 1024 * 1024 * 1024))

# 解析参数: 提取 --debug 标志, 其他位置参数
PROFILE_FLAG=""
POSITIONAL=()
for arg in "$@"; do
    case "$arg" in
        --debug|--dev|-d)
            PROFILE_FLAG="--debug"
            ;;
        *)
            POSITIONAL+=("$arg")
            ;;
    esac
done

# 默认 binary 和 mode
BINARY="${POSITIONAL[1]:-ele_bot_server}"
MODE="${POSITIONAL[0]:-all}"
# 处理单一参数情况 (e.g. ./deploy_rk3566.sh test_bd1)
if [[ "$MODE" != "build" && "$MODE" != "deploy" && "$MODE" != "all" ]]; then
    BINARY="$MODE"
    MODE="all"
fi

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

BINARY_TARGET="target/$TARGET/$PROFILE_DIR/$BINARY"

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
    if [[ ! -x "$BINARY_TARGET" ]]; then
        echo "错误: $BINARY_TARGET 不存在或不可执行, 请先 build"
        exit 1
    fi
}

# ---------- SSH 传输 ----------

# 优先用 ssh 密钥 (无密码), 失败则用 sshpass (有密码时)
# sshpass 仅在 RK_PASSWORD 设置或交互式询问后使用
deploy_binary() {
    local local_path="$1"
    local remote_path="$2"

    # 计算本地 sha256 用于后续校验
    local local_hash
    local_hash=$(shasum -a 256 "$local_path" | awk '{print $1}')
    echo "本地 sha256: $local_hash"

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
    remote_hash=$(ssh -o StrictHostKeyChecking=accept-new "$DEVICE" \
        "sha256sum '$remote_path' 2>/dev/null | awk '{print \$1}'")

    if [[ "$remote_hash" != "$local_hash" ]]; then
        echo "错误: sha256 不一致!"
        echo "  本地: $local_hash"
        echo "  设备: $remote_hash"
        echo "传输可能被截断, 请重试"
        exit 1
    fi
    echo "sha256 校验通过"
}

# ---------- build / deploy ----------

build_binary() {
    echo "=== 编译 $BINARY (profile: $PROFILE) ==="
    check_docker
    check_cross
    check_disk
    if ! HTTP_PROXY="$HTTP_PROXY" HTTPS_PROXY="$HTTPS_PROXY" \
         cross build $CROSS_PROFILE_FLAG --target "$TARGET" -p ele_bot_server --bin "$BINARY"; then
        echo "编译失败！"
        exit 1
    fi
    echo "=== 编译完成 ==="
}

deploy_step() {
    echo "=== 传输到设备 ==="
    check_binary_exists
    deploy_binary "$BINARY_TARGET" "$REMOTE_DIR/$BINARY"
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
        echo "未知参数: $MODE"
        echo "用法: $0 [build|deploy|all] [binary_name]"
        exit 1
        ;;
esac

echo ""
echo "在设备上运行："
echo "  普通模式: ./$BINARY"
echo "  测试人脸检测: TEST_RKNN=1 ./$BINARY"
echo "  测试指定模型: TEST_RKNN=1 RKNN_MODEL=./model/deepghs/yolo-face/yolo_face.rknn ./$BINARY"
echo "  测试指定图片: TEST_RKNN=1 TEST_IMAGE=./assets/images/test.png ./$BINARY"