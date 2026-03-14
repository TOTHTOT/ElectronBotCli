import numpy as np
import cv2
from rknn.api import RKNN

def post_process(outputs, conf_threshold=0.5, nms_threshold=0.45):
    """
    解析 YOLOv8n-face 输出 (1, 5, 8400)
    5 代表: [x_center, y_center, width, height, confidence]
    """
    data = np.squeeze(outputs[0])  # (5, 8400)
    data = data.T                 # (8400, 5)

    boxes = []
    confidences = []

    for row in data:
        score = row[4]
        if score > conf_threshold:
            # YOLOv8 输出是中心点坐标和宽高，且是基于 640x640 的
            x_center, y_center, w, h = row[0], row[1], row[2], row[3]
            
            # 转换为左上角坐标 [x1, y1, width, height]
            x = int(x_center - w / 2)
            y = int(y_center - h / 2)
            
            boxes.append([x, y, int(w), int(h)])
            confidences.append(float(score))

    # 使用 OpenCV 自带的 NMS 过滤掉重叠的框
    indices = cv2.dnn.NMSBoxes(boxes, confidences, conf_threshold, nms_threshold)
    
    final_results = []
    if len(indices) > 0:
        for i in indices.flatten():
            final_results.append({
                'box': boxes[i],
                'confidence': confidences[i]
            })
    return final_results

def main():
    model_path = './model/deepghs/yolo-face/yolo_face.rknn'
    img_path = './assets/images/figure1.png'
    output_res_path = './result.png'

    rknn = RKNN()

    # 1. 加载模型
    print('--> Loading RKNN model')
    ret = rknn.load_rknn(model_path)
    if ret != 0:
        print('Load RKNN model failed')
        return

    # 2. 初始化环境
    print('--> Init runtime environment on RK3566')
    ret = rknn.init_runtime(target='rk3566')
    if ret != 0:
        print('Init runtime failed')
        return

    # 3. 准备图片
    orig_img = cv2.imread(img_path)
    if orig_img is None:
        print(f"Error: Could not read image at {img_path}")
        return
        
    # 保存原始尺寸用于后续坐标映射
    img_h, img_w = orig_img.shape[:2]
    
    # 预处理：BGR -> RGB，并缩放到模型要求的 640x640
    img = cv2.cvtColor(orig_img, cv2.COLOR_BGR2RGB)
    img = cv2.resize(img, (640, 640))

    # 4. 执行推理
    print('--> Running inference...')
    outputs = rknn.inference(inputs=[img])

    # 5. 后处理
    print('--> Post-processing')
    results = post_process(outputs)

    # 6. 可视化绘制
    if len(results) == 0:
        print("!!! No faces detected.")
    else:
        print(f"Found {len(results)} faces:")
        for res in results:
            bx, by, bw, bh = res['box']
            conf = res['confidence']
            
            # 将坐标从 640x640 映射回原图尺寸
            x1 = int(bx * (img_w / 640))
            y1 = int(by * (img_h / 640))
            x2 = int((bx + bw) * (img_w / 640))
            y2 = int((by + bh) * (img_h / 640))

            print(f" - Face: [{x1}, {y1}, {x2}, {y2}], Conf: {conf:.2f}")

            # 画框和置信度
            cv2.rectangle(orig_img, (x1, y1), (x2, y2), (0, 255, 0), 2)
            cv2.putText(orig_img, f'{conf:.2f}', (x1, y1 - 10), 
                        cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 255, 0), 1)

    # 7. 保存结果
    cv2.imwrite(output_res_path, orig_img)
    print(f'--> Result saved to: {output_res_path}')

    rknn.release()

if __name__ == '__main__':
    main()