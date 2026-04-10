import numpy as np
from PIL import Image, ImageDraw
import onnxruntime as ort

# 1. 路径配置
model_path = "/Users/yangyihui/.cache/huggingface/hub/models--deepghs--yolo-face/snapshots/e3662574830c534dfcc9c3b7ea4d89272f8aae4e/yolov8n-face/model.onnx"
image_path = "assets/images/figure1.png"
output_path = "assets/images/result.png"

# 2. 加载模型并处理动态维度
session = ort.InferenceSession(model_path)
input_cfg = session.get_inputs()[0]
input_name = input_cfg.name
shape = input_cfg.shape

h_model = shape[2] if isinstance(shape[2], int) else 640
w_model = shape[3] if isinstance(shape[3], int) else 640

print("=== 1. Model Info ===")
print(f"Input Name: {input_name}")
print(f"Target Shape: {w_model}x{h_model}")

# 3. 预处理 (Letterbox)
orig_img = Image.open(image_path).convert("RGB")
w_orig, h_orig = orig_img.size

# 计算缩放比例
scale = min(w_model / w_orig, h_model / h_orig)
nw, nh = int(w_orig * scale), int(h_orig * scale)

print(f"\n=== 2. Preprocessing ===")
print(f"Original Size: {w_orig}x{h_orig}")
print(f"Letterbox Size: {nw}x{nh} (Scale: {scale:.4f})")

# 缩放并填充画布
img_resized = orig_img.resize((nw, nh), Image.Resampling.LANCZOS)
canvas = Image.new("RGB", (w_model, h_model), (114, 114, 114))
canvas.paste(img_resized, (0, 0))

# NCHW 转换
img_data = np.array(canvas).astype(np.float32) / 255.0
img_data = img_data.transpose(2, 0, 1)
input_tensor = np.expand_dims(img_data, axis=0)

# 4. 执行推理
outputs = session.run(None, {input_name: input_tensor})
# data shape: (5, 8400)
data = outputs[0][0]

print(f"\n=== 3. Inference Output ===")
print(f"Raw Output Shape: {outputs[0].shape}")
print(f"Scores Row (Index 4) - Min: {data[4].min():.4f}, Max: {data[4].max():.4f}")

# 5. 后处理 (NMS)
CONF_THRESHOLD = 0.35
IOU_THRESHOLD = 0.45

def nms(boxes, scores, iou_thres):
    if len(boxes) == 0: return []
    x1, y1, x2, y2 = boxes[:, 0], boxes[:, 1], boxes[:, 2], boxes[:, 3]
    areas = (x2 - x1) * (y2 - y1)
    order = scores.argsort()[::-1]
    keep = []
    while order.size > 0:
        i = order[0]
        keep.append(i)
        xx1 = np.maximum(x1[i], x1[order[1:]])
        yy1 = np.maximum(y1[i], y1[order[1:]])
        xx2 = np.minimum(x2[i], x2[order[1:]])
        yy2 = np.minimum(y2[i], y2[order[1:]])
        w = np.maximum(0.0, xx2 - xx1)
        h = np.maximum(0.0, yy2 - yy1)
        inter = w * h
        ovr = inter / (areas[i] + areas[order[1:]] - inter + 1e-6)
        order = order[np.where(ovr <= iou_thres)[0] + 1]
    return keep

# 筛选置信度
mask = data[4] > CONF_THRESHOLD
candidates = data[:, mask].T

final_boxes, final_scores = [], []
if len(candidates) > 0:
    # xywh -> xyxy
    boxes = np.zeros_like(candidates[:, :4])
    boxes[:, 0] = candidates[:, 0] - candidates[:, 2] / 2
    boxes[:, 1] = candidates[:, 1] - candidates[:, 3] / 2
    boxes[:, 2] = candidates[:, 0] + candidates[:, 2] / 2
    boxes[:, 3] = candidates[:, 1] + candidates[:, 3] / 2

    scores = candidates[:, 4]
    keep = nms(boxes, scores, IOU_THRESHOLD)
    final_boxes, final_scores = boxes[keep], scores[keep]

print(f"\n=== 4. Post-processing ===")
print(f"Candidates > {CONF_THRESHOLD}: {len(candidates)}")
print(f"Final Boxes after NMS: {len(final_boxes)}")

# 6. 绘图与坐标映射
draw = ImageDraw.Draw(orig_img)
for i, (box, score) in enumerate(zip(final_boxes, final_scores)):
    # 映射回原图并纠正 PIL 可能的 x1 > x2 错误
    x1, y1, x2, y2 = box / scale
    left, right = sorted([x1, x2])
    top, bottom = sorted([y1, y2])

    # 打印每个检测到的框的调试信息
    print(f"  [Face {i}] Score: {score:.4f} | Box: [{int(left)}, {int(top)}, {int(right)}, {int(bottom)}]")

    draw.rectangle([left, top, right, bottom], outline="#00FF88", width=4)
    draw.text((left, top - 15), f"{score:.2f}", fill="#00FF88")

orig_img.save(output_path)
print(f"\n=== 5. Finished ===")
print(f"Result saved to: {output_path}")