import onnx
import sys

# 优先读取命令行参数，否则才报错
if len(sys.argv) < 2:
    print("错误: 请提供 ONNX 文件路径")
    sys.exit(1)

model_path = sys.argv[1]
print(f"正在加载模型: {model_path}")

try:
    model = onnx.load(model_path)
    for input in model.graph.input:
        # 提取维度信息
        shape = [dim.dim_value if dim.dim_value > 0 else "Dynamic" for dim in input.type.tensor_type.shape.dim]
        print(f"\n输入节点: {input.name}")
        print(f"输入形状 (Shape): {shape}")

    for output in model.graph.output:
        shape = [dim.dim_value if dim.dim_value > 0 else "Dynamic" for dim in input.type.tensor_type.shape.dim]
        print(f"\n输出节点: {output.name}")
        print(f"输出形状 (Shape): {shape}")
except Exception as e:
    print(f"解析失败: {e}")