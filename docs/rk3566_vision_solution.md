# RK3566 NPU 视觉方案

## 需求
实现以下视觉功能，调用 RK3566 NPU：
1. 表情识别 (Expression Recognition)
2. 视线注视 (Gaze/Eye Tracking)
3. 人脸位置 (Face Location)

---

## 模型选择: YuNet

### 为什么选择 YuNet？

| 特性 | 说明 |
|------|------|
| 输出 | 5 个关键点 (左眼、右眼、鼻子、左嘴角、右嘴角) |
| 格式 | ONNX (可直接下载) |
| 用途 | 一个模型搞定人脸检测 + 关键点 |

### 关键点对应功能

| 需求 | 实现方式 | 难度 |
|------|----------|------|
| 人脸位置 | 人脸框中心坐标 | 简单 |
| 视线注视 | 双眼关键点坐标 → 视线方向 | 中等 |
| 表情识别 | 关键点几何特征 → 微笑/张嘴 | 简单 |

---

## 视觉方案架构

```
摄像头图像
    ↓
[YuNet] → 人脸框坐标 + 5个关键点
    ↓
裁剪人脸区域
    ↓
    ├─→ [表情识别模型] → 表情类别
    └─→ [视线估计模型] → 视线方向
```

**分工**：
- **YuNet**: 负责人脸检测 + 关键点（输出人脸位置）
- **其他模型**: 负责表情分类、视线估计（输入裁剪后的人脸图像）

---

## YuNet 输出

### 关键点索引
```
0: 左眼 (left_eye)
1: 右眼 (right_eye)
2: 鼻子 (nose)
3: 左嘴角 (left_corner)
4: 右嘴角 (right_corner)
```

### 人脸位置计算

```rust
// 人脸位置 (归一化坐标 0-1)
face_x = (bbox.x + bbox.width / 2) / image_width
face_y = (bbox.y + bbox.height / 2) / image_height

// 裁剪人脸区域
face_image = original_image[bbox.y : bbox.y + bbox.height,
                           bbox.x : bbox.x + bbox.width]
```

---

## 表情识别 (使用独立模型)

### 模型选择

| 模型 | 用途 | 来源 |
|------|------|------|
| Emotion FerPlus | 8 种表情分类 | ONNX Model Zoo |
| miniFER | 轻量级表情 | 自训练 |

### 输入输出

```rust
// 输入: 裁剪后的人脸图像 (64x64 或 112x112)
// 输出: 表情类别

enum Expression {
    Neutral,   // 中性
    Happy,     // 开心
    Sad,       // 悲伤
    Surprise,  // 惊讶
    Fear,      // 害怕
    Disgust,   // 厌恶
    Anger,     // 生气
}
```

---

## 视线估计 (使用独立模型)

### 模型选择

| 模型 | 用途 | 来源 |
|------|------|------|
| OpenGaze | 视线估计 | GitHub |
| MPIIFaceGaze | 视线估计 | 自训练 |

### 输入输出

```rust
// 输入: 裁剪后的人脸图像 (224x224)
// 输出: 视线方向

struct Gaze {
    x: f32,  // -1 (左) ~ 1 (右)
    y: f32,  // -1 (上) ~ 1 (下)
}
```

---

## 模型下载

### YuNet ONNX 模型

```bash
# FP32 版本 (精度高，较大)
wget https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx

# INT8 版本 (推荐，小巧适合 RK3566)
wget https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar_int8.onnx
```

### 模型文件
- [OpenCV Zoo 官方仓库](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet)
- 包含: FP32 / INT8 / INT8-BQ 三种版本

---

## 其他模型下载

### 表情识别: Emotion FerPlus

| 项目 | 链接 |
|------|------|
| 官方仓库 | https://github.com/onnx/models/tree/main/validated/vision/body_analysis/emotion_ferplus |
| 模型下载 | https://github.com/onnx/models/raw/main/validated/vision/body_analysis/emotion_ferplus/model/emotion-ferplus-8.onnx |

```bash
# 下载 (34MB)
wget https://github.com/onnx/models/raw/main/validated/vision/body_analysis/emotion_ferplus/model/emotion-ferplus-8.onnx

# INT8 量化版本 (19MB，更小)
wget https://github.com/onnx/models/raw/main/validated/vision/body_analysis/emotion_ferplus/model/emotion-ferplus-12-int8.onnx
```

**输出类别**: Neutral, Happiness, Surprise, Fear, Disgust, Anger, Sadness

---

### 视线估计: MobileGaze

| 项目 | 链接 |
|------|------|
| GitHub | https://github.com/yakhyo/gaze-estimation |
| UniFace (多功能) | https://github.com/yakhyo/uniface |

```bash
# 克隆项目查看模型
git clone https://github.com/yakhyo/gaze-estimation.git
```

**模型选项**:
- MobileNetV2 (轻量)
- ResNet-18
- MobileOne

> 注意: 这些模型需要转换为 RKNN 格式才能在 NPU 上运行。

---

## 实现步骤

### 1. 环境准备
- 安装 rknn-toolkit2
- 下载 YuNet ONNX 模型
- 转换为 RKNN 格式 (如果需要)

### 2. 添加依赖
```toml
# Cargo.toml
rknn-toolkit2 = "2.0"
```

### 3. 创建视觉模块
新建 `src/vision/mod.rs`:

```
src/vision/
├── mod.rs      # 主模块
├── yunet.rs    # YuNet 推理
├── emotion.rs  # 表情识别
└── gaze.rs     # 视线估计
```

### 4. 处理流程

```rust
pub struct VisionResult {
    pub face_detected: bool,
    pub face_x: f32,      // 人脸中心 X (0-1)
    pub face_y: f32,      // 人脸中心 Y (0-1)
    pub expression: Expression,  // 表情类别
    pub gaze: Gaze,             // 视线方向
}

// 处理流程
fn process(image: &Mat) -> VisionResult {
    // 1. YuNet 检测人脸
    let (bboxes, keypoints) = yunet.detect(image)?;

    if bboxes.is_empty() {
        return VisionResult { face_detected: false, ... };
    }

    // 2. 获取人脸位置
    let face = &bboxes[0];
    let face_x = (face.x + face.width/2) / image.width;
    let face_y = (face.y + face.height/2) / image.height;

    // 3. 裁剪人脸区域
    let face_image = crop_face(image, face);

    // 4. 表情识别
    let expression = emotion_model.predict(&face_image);

    // 5. 视线估计
    let gaze = gaze_model.predict(&face_image);

    VisionResult { face_detected: true, face_x, face_y, expression, gaze }
}
```

### YuNet 输出格式

```
输出: [人脸数, (x1, y1, x2, y2, 关键点1.x, 关键点1.y, ...关键点5.x, 关键点5.y, 置信度)]
```

### 裁剪人脸

```rust
// 根据人脸框裁剪原始图像
fn crop_face(image: &Mat, bbox: &Rect) -> Mat {
    image.roi(bbox).clone()
}
```

---

## 验证步骤

1. [ ] 下载 YuNet ONNX 模型
2. [ ] 下载表情/视线模型 (Emotion FerPlus / OpenGaze)
3. [ ] 转换所有模型为 RKNN 格式
4. [ ] 运行推理，确认输出
5. [ ] 测试:
   - [ ] 人脸检测正常
   - [ ] 人脸位置坐标正确
   - [ ] 表情识别正确
   - [ ] 视线方向正确

---

## 备选方案

### 人脸检测: RetinaFace
- 同样输出 5 个关键点
- RKNN Model Zoo 有预编译版本
- 下载: https://console.zbox.filez.com/l/8ufwtG (key: rknn)

### 轻量级表情: miniFER
- 更小的模型，适合嵌入式
- 需要自训练或找预训练模型
