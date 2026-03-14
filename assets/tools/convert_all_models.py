#!/usr/bin/env python3
import os
import cv2
from rknn.api import RKNN

# 测试图片目录
TEST_IMAGES_DIR = '../images'
OUTPUT_DIR = './images_640'
TARGET_SIZE = (640, 640)

MODELS = [
    {
        'name': 'yolo_face',
        'onnx': '/home/radxa/.cache/huggingface/hub/models--deepghs--yolo-face/snapshots/e3662574830c534dfcc9c3b7ea4d89272f8aae4e/yolov8n-face/model.onnx',
        'rknn': './model/deepghs/yolo-face/yolo_face_int8.rknn',
        'inputs': ['images'],
        'input_size_list': [[1, 3, 640, 640]],
    },
]


def resize_test_images():
    """将测试图片 resize 为 640x640，返回 numpy 数组列表"""
    os.makedirs(OUTPUT_DIR, exist_ok=True)

    valid_exts = ('.jpg', '.jpeg', '.png', '.bmp')
    files = [f for f in os.listdir(TEST_IMAGES_DIR) if f.lower().endswith(valid_exts)]

    print(f"Found {len(files)} test images")

    dataset = []
    for f in files:
        src = os.path.join(TEST_IMAGES_DIR, f)
        dst = os.path.join(OUTPUT_DIR, f)

        img = cv2.imread(src)
        if img is None:
            print(f"  [SKIP] {f} - cannot read")
            continue

        # Resize to 640x640
        img_resized = cv2.resize(img, TARGET_SIZE, interpolation=cv2.INTER_LINEAR)
        # BGR -> RGB
        img_rgb = cv2.cvtColor(img_resized, cv2.COLOR_BGR2RGB)
        # 转换为 numpy 数组 (H, W, C)
        import numpy as np
        img_np = np.array(img_rgb, dtype=np.uint8)
        dataset.append(img_np)

        cv2.imwrite(dst, img_resized)
        orig_h, orig_w = img.shape[:2]
        print(f"  [OK] {f}: {orig_w}x{orig_h} -> 640x640")

    print(f"Dataset prepared: {len(dataset)} images")
    return dataset


def convert_model(model, dataset=None):
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

        # 配置
        rknn.config(
            mean_values=[[0, 0, 0]],
            std_values=[[255, 255, 255]],
            target_platform='rk3566',
            quantized_dtype='w8a8',  # int8 量化 (weight 8bit, activation 8bit)
        )

        ret = rknn.load_onnx(
            model=onnx_path,
            inputs=model.get('inputs'),
            input_size_list=model.get('input_size_list')
        )
        if ret != 0:
            print(f"Error: Load ONNX failed!")
            return False

        print(f"Building RKNN model (int8 quantization)...")

        # 启用量化
        ret = rknn.build(do_quantization=True, dataset=dataset)

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
        import traceback
        traceback.print_exc()
        return False


def main():
    print("Starting model conversion with int8 quantization...")
    print(f"Test images dir: {TEST_IMAGES_DIR}")

    # 准备测试数据集
    dataset = resize_test_images()

    success_count = 0
    for model in MODELS:
        if convert_model(model, dataset):
            success_count += 1

    print(f"\n{'='*50}")
    print(f"Conversion complete: {success_count}/{len(MODELS)} succeeded")
    print(f"{'='*50}")


if __name__ == '__main__':
    main()
