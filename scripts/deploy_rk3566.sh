#!/bin/bash

# 编译并部署到 RK3566 设备

TARGET="aarch64-unknown-linux-gnu"
DEVICE="radxa@192.168.2.202"
REMOTE_DIR="~/ElectronBotCli"

echo "=== 开始编译 ==="
if ! http_proxy=http://192.168.2.147:7890 \
     https_proxy=http://192.168.2.147:7890 \
     cross build --release --target $TARGET; then
    echo "编译失败！"
    exit 1
fi

echo "=== 编译成功 ==="
echo "=== 部署到设备 ==="
if ! scp target/$TARGET/release/ele_bot $DEVICE:$REMOTE_DIR/ele_bot; then
    echo "部署失败！"
    exit 1
fi

# 复制 assets 目录（如果存在）
#if [ -d "assets" ]; then
#    echo "=== 复制 assets 目录 ==="
#    if ! scp -r assets $DEVICE:$REMOTE_DIR/; then
#        echo "复制 assets 失败！"
#        exit 1
#    fi
#fi

echo "=== 部署完成 ==="
echo ""
echo "在设备上运行："
echo "  普通模式: ./ele_bot"
echo "  测试人脸检测: TEST_RKNN=1 ./ele_bot"
echo "  测试指定模型: TEST_RKNN=1 RKNN_MODEL=./model/deepghs/yolo-face/yolo_face.rknn ./ele_bot"
echo "  测试指定图片: TEST_RKNN=1 TEST_IMAGE=./assets/images/test.png ./ele_bot"
