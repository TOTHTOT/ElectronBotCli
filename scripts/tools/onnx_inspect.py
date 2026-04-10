#!/usr/bin/env python3
"""ONNX 模型算子分析工具"""

import sys
import onnx
from collections import Counter

def inspect_onnx(model_path: str):
    if len(sys.argv) < 2:
        print(f"用法: python {sys.argv[0]} <模型路径>")
        print(f"示例: python {sys.argv[0]} model.onnx")
        sys.exit(1)

    print(f"=== ONNX 模型算子分析 ===")
    print(f"模型: {model_path}\n")

    model = onnx.load(model_path)
    graph = model.graph

    # 收集所有算子
    operators = [node.op_type for node in graph.node]

    # 统计算子
    op_counts = Counter(operators)

    print("--- 算子统计 (按使用次数) ---")
    for op, count in op_counts.most_common():
        print(f"  {op}: {count}")

    print(f"\n--- 所有算子类型 ({len(op_counts)} 种) ---")
    for op in sorted(op_counts.keys()):
        print(f"  - {op}")

    # 打印模型信息
    print("\n--- 模型信息 ---")
    print(f"  输入数量: {len(graph.input)}")
    for inp in graph.input:
        shape = [d.dim_value if d.dim_value > 0 else d.dim_param for d in inp.type.tensor_type.shape.dim]
        print(f"    - {inp.name}: {shape}")

    print(f"  输出数量: {len(graph.output)}")
    for out in graph.output:
        print(f"    - {out.name}")

    print(f"  节点数量: {len(graph.node)}")

    if model.ir_version:
        print(f"  IR 版本: {model.ir_version}")

    if model.producer_name:
        print(f"  生产者: {model.producer_name}")

if __name__ == "__main__":
    inspect_onnx(sys.argv[1])
