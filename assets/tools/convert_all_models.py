#!/usr/bin/env python3
import os
from rknn.api import RKNN

MODELS = [
    {
        'name': 'sense_voice',
        'onnx': '/home/radxa/.cache/huggingface/hub/models--csukuangfj--sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/snapshots/2365baeacb507f821a0c8120fcee3d484dba7a07/model.int8.onnx',
        'rknn': './model/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/model.int8.rknn',
        'inputs': ['x', 'x_length', 'language', 'text_norm'],
        'input_size_list': [[1, 300, 560], [1], [1], [1]],
    },
    {
        'name': 'silero_vad',
        'onnx': '/home/radxa/.cache/huggingface/hub/models--deepghs--silero-vad-onnx/snapshots/193243f7d961b15e6de789d3f90cb0ee867e7b62/silero_vad.onnx',
        'rknn': '/home/radxa/model/deepghs/silero-vad-onnx/silero_vad.rknn',
    },
    {
        'name': 'yolo_face',
        'onnx': '/home/radxa/.cache/huggingface/hub/models--deepghs--yolo-face/snapshots/e3662574830c534dfcc9c3b7ea4d89272f8aae4e/yolov8n-face/model.onnx',
        'rknn': './model/deepghs/yolo-face/yolo_face.rknn',
        'inputs': ['images'],
        'input_size_list': [[1, 3, 640, 640]],
    },
]


def convert_model(model):
    """转换单个模型"""
    name = model['name']
    onnx_path = model['onnx']
    rknn_path = model['rknn']

    print(f"\n{'='*50}")
    print(f"Converting: {name}")
    print(f"{'='*50}")

    # 创建输出目录
    os.makedirs(os.path.dirname(rknn_path), exist_ok=True)

    if not os.path.exists(onnx_path):
        print(f"Error: ONNX model not found: {onnx_path}")
        return False

    try:
        rknn = RKNN()
        print(f"Loading ONNX: {onnx_path}")
        rknn.config(mean_values=[[0, 0, 0]], std_values=[[255, 255, 255]], target_platform='rk3566')
        ret = rknn.load_onnx(
                    model=onnx_path,
                    inputs=model.get('inputs'),
                    input_size_list=model.get('input_size_list'))
        if ret != 0:
            print(f"Error: Load ONNX failed!")
            return False

        print(f"Building RKNN model...")
        ret = rknn.build(do_quantization=False)
        if ret != 0:
            print(f"Error: Build failed!")
            return False

        print(f"Exporting RKNN: {rknn_path}")
        ret = rknn.export_rknn(rknn_path)
        if ret != 0:
            print(f"Error: Export failed!")
            return False

        print(f"Success: {name} -> {rknn_path}")
        return True
    except Exception as e:
        print(f"Error: {e}")
        return False


def main():
    print("Starting model conversion...")

    success_count = 0
    for model in MODELS:
        if convert_model(model):
            success_count += 1

    print(f"\n{'='*50}")
    print(f"Conversion complete: {success_count}/{len(MODELS)} succeeded")
    print(f"{'='*50}")


if __name__ == '__main__':
    main()
