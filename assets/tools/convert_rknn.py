#!/usr/bin/env python3
"""Convert YOLOv8 face detection ONNX model to RKNN with int8 quantization."""

import os
import onnx
from rknn.api import RKNN

ONNX_MODEL = 'model.onnx'
RKNN_MODEL = 'model.rknn'
DATASET = './dataset.txt'

# YOLOv8 common input names: 'images', 'input', 'x', 'input.1'
# Check your model's actual input name below
MODEL_INPUT_NAME = None  # Will be auto-detected

def get_onnx_input_name(model_path):
    """Get the actual input name from ONNX model."""
    model = onnx.load(model_path)
    inputs = model.graph.input
    input_names = [inp.name for inp in inputs]
    print(f"ONNX model inputs: {input_names}")
    return input_names[0] if input_names else 'images'

def convert_to_rknn():
    global MODEL_INPUT_NAME

    # Auto-detect input name if not set
    if MODEL_INPUT_NAME is None:
        MODEL_INPUT_NAME = get_onnx_input_name(ONNX_MODEL)
        print(f"Using input name: {MODEL_INPUT_NAME}")

    # Create RKNN object
    rknn = RKNN(verbose=True)

    # Config model with int8 quantization
    print('--> Config model')
    rknn.config(
        mean_values=[[0, 0, 0]],
        std_values=[[255, 255, 255]],
        target_platform='rk3566',
        quantized_dtype='w8a8',  # int8 quantization (weight 8bit, activation 8bit)
    )
    print('done')

    # Load ONNX model
    print('--> Loading ONNX model')
    ret = rknn.load_onnx(
        model=ONNX_MODEL,
        inputs=[MODEL_INPUT_NAME],
        input_size_list=[[1, 3, 640, 640]]
    )
    if ret != 0:
        print('Error: Load ONNX failed!')
        return False
    print('done')

    # Build model with quantization
    print('--> Building model with int8 quantization')
    ret = rknn.build(do_quantization=True, dataset=DATASET)
    if ret != 0:
        print('Error: Build model failed!')
        return False
    print('done')

    # Export RKNN model
    print('--> Export RKNN model')
    ret = rknn.export_rknn(RKNN_MODEL)
    if ret != 0:
        print('Error: Export RKNN model failed!')
        return False
    print('done')

    print(f'\nSuccess! RKNN model saved to: {RKNN_MODEL}')
    rknn.release()
    return True

if __name__ == '__main__':
    # Check if dataset exists
    if not os.path.exists(DATASET):
        print(f'Warning: {DATASET} not found!')
        print('Please create a dataset.txt with image paths for quantization.')
        print('Example:')
        print('  /path/to/image1.jpg')
        print('  /path/to/image2.jpg')
        print('  ...')
        print('\nCreating empty dataset.txt for now...')
        with open(DATASET, 'w') as f:
            f.write('')

    convert_to_rknn()