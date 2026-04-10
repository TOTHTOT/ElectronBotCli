# ElectronBotCli

- 这是[ElectronBot](https://github.com/peng-zhihui/ElectronBot.git)的命令行上位机, 使用rust编写, 支持跨平台运行.

- 现计划是通过rk3566(2G+16G)的核心板(55mm*40mm)+外扩底板以机器人背包的形式挂载, 将原先的usb尾插小板功能移植到底板.

## 包含功能

1. [x] 移植原上位机功能, 包含: usb的cdc通信, 舵机控制, 屏幕控制, 目前屏幕刷新能做到20fps
2. [x] 支持表情显示, 采用[RoboEyes](https://github.com/FluxGarage/RoboEyes/tree/main)的实现方式, 动态生成表情
3. [x] 使用`sherpa-onnx`调用usb麦克风实现asr和tts
4. [ ] 视觉方面需要的功能
    - [ ] 实现表情识别, 模仿表情
    - [x] 人脸位置, 跟随人脸转动
    - 区分平台, pc使用`ort`推理, rk3566使用`rknn`推理模型
5. [ ] 语音对话, 预计采用哔哔声作为应答, 同时支持文字转语音, 只有基础功能, 运行逻辑没实现
6. [ ] 接入llm然后通过提示词让llm控制身体, 区分在线和离线
   - 目前离线模型推理很慢, 离线时只做简单表情处理, 在线模型才支持肢体动作
 
## 长期计划
- 在添加完基本功能后希望机器人能够自主移动, 通过识别aruco码来让机器人回到充电桩.

## 使用方法
- 拿到对应的ele_bot程序后直接运行会在`hugging face`的默认目录下载模型, 然后程序退出手动调用`convert_all_models.py`进行模型转换再次启动程序即可.

### 编译
- ~~当运行在pc平台是, 使用的推理框架是`onnx`, 这时需要手动安装运行时.~~
   ```shell
   brew install onnxruntime # mac
   sudo apt install libonnxruntime-dev # ubuntu
   winget install Microsoft.OnnxRuntime # Windows
   ```
- 在安装了`docker`的情况下使用`./scripts/deploy_rk3566.sh`就能编译出rk3566的程序

- 常规直接`cargo run release`即可

### 运行
1. ~~配置usb的udev规则~~
    ```shell
    sudo vim /etc/udev/rules.d/99-electronbot.rules
    # 文件内输入
    SUBSYSTEM=="usb", ATTR{idVendor}=="xxxx", ATTR{idProduct}=="yyyy", MODE="0666"
    
    #保存后重新加载规则
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    ```
2. 部署到到rk3566
```shell
# cross+docker 编译程序, cross 配置参考 Cross.toml
cross build --target aarch64-unknown-linux-gnu --release
# 发送编译好的程序
scp target/aarch64-unknown-linux-gnu/release/ele_bot  radxa@192.168.2.202:~/ElectronBotCli
# 同步资源文件
scp target/aarch64-unknown-linux-gnu/release/libsherpa-onnx-c-api.so target/aarch64-unknown-linux-gnu/release/libonnxruntime.so radxa@192.168.2.202:~/
scp assets/tools/convert_all_models.py  radxa@192.168.2.202:~/ElectronBotCli
```

### 待优化

1. [ ] 优化人脸识别方面的, 减少内存拷贝并持久化一些变量, 免得重复开辟大量空间.
2. [x] 在rk3566情况下使用硬件rga进行旋转 缩放 格式转换.

## 备注
