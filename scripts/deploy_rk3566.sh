#!/bin/bash

# 编译并部署到 RK3566 设备
# 用法:
#   ./deploy_rk3566.sh          - 编译并传输（默认）
#   ./deploy_rk3566.sh build    - 只编译
#   ./deploy_rk3566.sh deploy   - 只传输

TARGET="aarch64-unknown-linux-gnu"
DEVICE="radxa@192.168.2.202"
REMOTE_DIR="~/ElectronBotCli"

# 解析参数
MODE="${1:-all}"

case "$MODE" in
    build)
        echo "=== 只编译模式 ==="
        if ! http_proxy=http://192.168.2.147:7890 \
             https_proxy=http://192.168.2.147:7890 \
             cross build --release --target $TARGET; then
            echo "编译失败！"
            exit 1
        fi
        echo "=== 编译完成 ==="
        ;;
    deploy)
        echo "=== 只传输模式 ==="
        if ! scp target/$TARGET/release/ele_bot $DEVICE:$REMOTE_DIR/ele_bot; then
            echo "传输失败！"
            exit 1
        fi
        echo "=== 传输完成 ==="
        ;;
    all|"")
        echo "=== 编译并传输模式 ==="
        echo "=== 开始编译 ==="
        if ! http_proxy=http://192.168.2.147:7890 \
             https_proxy=http://192.168.2.147:7890 \
             cross build --release --target $TARGET; then
            echo "编译失败！"
            exit 1
        fi

        echo "=== 编译成功 ==="
        echo "=== 传输到设备 ==="
        if ! scp target/$TARGET/release/ele_bot $DEVICE:$REMOTE_DIR/ele_bot; then
            echo "传输失败！"
            exit 1
        fi
        echo "=== 传输完成 ==="
        ;;
    *)
        echo "未知参数: $MODE"
        echo "用法: $0 [build|deploy]"
        exit 1
        ;;
esac

echo ""
echo "在设备上运行："
echo "  普通模式: ./ele_bot"
echo "  测试人脸检测: TEST_RKNN=1 ./ele_bot"
echo "  测试指定模型: TEST_RKNN=1 RKNN_MODEL=./model/deepghs/yolo-face/yolo_face.rknn ./ele_bot"
echo "  测试指定图片: TEST_RKNN=1 TEST_IMAGE=./assets/images/test.png ./ele_bot"
