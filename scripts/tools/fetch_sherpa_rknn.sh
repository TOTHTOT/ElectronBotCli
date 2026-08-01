#!/usr/bin/env bash
# 下载 sherpa-onnx rknn 预编译共享库到 assets/lib/sherpa-onnx-rknn/
#
# 这些 .so (~39MB) 不进 git (.gitignore 已排除), 新机器 clone 后跑本脚本即可.
# 用途:
#   1. Dockerfile.cross COPY 进 cross 镜像, 编译时经 SHERPA_ONNX_LIB_DIR 链接
#   2. 编译产物旁的运行时库, deploy_rk3566.sh 会一起推到设备
#
# 用法:
#   ./scripts/tools/fetch_sherpa_rknn.sh            # 下载默认版本
#   ./scripts/tools/fetch_sherpa_rknn.sh 1.13.4     # 指定版本
#   FORCE=1 ./scripts/tools/fetch_sherpa_rknn.sh    # 已存在也强制重下
#
# 版本必须和 Cargo.toml 里 sherpa-onnx crate 版本一致, 升级 crate 时
# 重跑本脚本替换预编译库. 走代理: HTTP_PROXY/HTTPS_PROXY 环境变量即可.
set -euo pipefail

VERSION="${1:-1.13.4}"
PKG="sherpa-onnx-v${VERSION}-rknn-linux-aarch64-shared"
URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/v${VERSION}/${PKG}.tar.bz2"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="$ROOT/assets/lib/sherpa-onnx-rknn"
NEEDED=(libonnxruntime.so libsherpa-onnx-c-api.so libsherpa-onnx-cxx-api.so)

if [[ -d "$DEST" && "${FORCE:-0}" != "1" ]]; then
    missing=0
    for so in "${NEEDED[@]}"; do
        [[ -f "$DEST/lib/$so" ]] || missing=1
    done
    if [[ "$missing" == "0" ]]; then
        echo "已存在且完整: $DEST (FORCE=1 可强制重下)"
        exit 0
    fi
    echo "目录存在但缺文件, 重新下载..."
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "下载 $URL"
curl -fL --retry 3 -o "$TMP/pkg.tar.bz2" "$URL"

echo "解压..."
tar -xjf "$TMP/pkg.tar.bz2" -C "$TMP"
# 压缩包顶层目录名可能带版本号, 找包含 lib/libonnxruntime.so 的那层
SRC="$(dirname "$(find "$TMP" -name libonnxruntime.so -path '*/lib/*' | head -1)")/.."
SRC="$(cd "$SRC" && pwd)"

rm -rf "$DEST"
mkdir -p "$DEST"
# 只保留 lib (运行时/链接用) 和 bin (调试工具), include 用不上
cp -R "$SRC/lib" "$DEST/lib"
[[ -d "$SRC/bin" ]] && cp -R "$SRC/bin" "$DEST/bin"

for so in "${NEEDED[@]}"; do
    [[ -f "$DEST/lib/$so" ]] || { echo "错误: 解压后缺少 lib/$so"; exit 1; }
done

echo "完成: $DEST"
ls -lh "$DEST/lib/"
