import os
from rknn.api import RKNN

ONNX_MODEL = '/home/radxa/.cache/huggingface/hub/models--deepghs--yolo-face/snapshots/e3662574830c534dfcc9c3b7ea4d89272f8aae4e/yolov8n-face/model.onnx'
RKNN_MODEL = '/home/radxa/yolov8n-face.rknn'

if not os.path.exists(ONNX_MODEL):
    print(f'Error: ONNX model not found: {ONNX_MODEL}')
    exit(1)

print(f'Loading ONNX model: {ONNX_MODEL}')

rknn = RKNN()

# 配置转换参数，设置固定输入尺寸
rknn.config(
    target_platform='rk3566'
)

# 加载 ONNX 模型，指定输入尺寸
print('Loading ONNX...')
ret = rknn.load_onnx(
    ONNX_MODEL,
    inputs=['images'],
    input_size_list=[[1, 3, 640, 640]]
)
if ret != 0:
    print(f'Load ONNX model failed!')
    exit(1)

# 构建 RKNN 模型 (不使用量化)
print('Building RKNN model (this may take a while)...')
ret = rknn.build(do_quantization=False)
if ret != 0:
    print(f'Build RKNN model failed!')
    exit(1)

# 导出 RKNN 模型
print(f'Exporting RKNN model to: {RKNN_MODEL}')
ret = rknn.export_rknn(RKNN_MODEL)
if ret != 0:
    print(f'Export RKNN model failed!')
    exit(1)

print('Conversion completed successfully!')
print(f'RKNN model saved to: {RKNN_MODEL}')
