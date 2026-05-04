#!/bin/bash

# 编译并部署到 RK3566 设备
# 用法:
#   ./deploy_rk3566.sh                    - 编译并传输 ele_bot（默认）
#   ./deploy_rk3566.sh build              - 只编译 ele_bot
#   ./deploy_rk3566.sh deploy              - 只传输 ele_bot
#   ./deploy_rk3566.sh test_bd1            - 编译并传输 test_bd1
#   ./deploy_rk3566.sh build test_bd1      - 只编译 test_bd1
#   ./deploy_rk3566.sh deploy test_bd1     - 只传输 test_bd1

TARGET="aarch64-unknown-linux-gnu"
DEVICE="radxa@192.168.2.159"
REMOTE_DIR="~/ElectronBotCli"

# 默认 binary
BINARY="ele_bot"

# 解析参数
MODE="${1:-all}"
if [[ "$2" != "" ]]; then
    BINARY="$2"
fi

# 处理单一参数情况
if [[ "$MODE" != "build" && "$MODE" != "deploy" && "$MODE" != "all" ]]; then
    BINARY="$MODE"
    MODE="all"
fi

BINARY_TARGET="target/$TARGET/release/$BINARY"

build_binary() {
    echo "=== 编译 $BINARY ==="
    if ! http_proxy=http://192.168.2.147:7890 \
         https_proxy=http://192.168.2.147:7890 \
         cross build --release --target $TARGET --bin $BINARY; then
        echo "编译失败！"
        exit 1
    fi
    echo "=== 编译完成 ==="
}

deploy_binary() {
    echo "=== 传输到设备 ==="
    if ! sshpass -p "radxa" scp -o StrictHostKeyChecking=no "$BINARY_TARGET" "$DEVICE:$REMOTE_DIR/$BINARY"; then
        echo "传输失败！"
        exit 1
    fi
    echo "=== 传输完成 ==="
}

case "$MODE" in
    build)
        build_binary
        ;;
    deploy)
        deploy_binary
        ;;
    all|"")
        build_binary
        deploy_binary
        ;;
    *)
        echo "未知参数: $MODE"
        echo "用法: $0 [build|deploy|all] [binary_name]"
        exit 1
        ;;
esac
