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
5. [ ] 语音对话, 预计采用哔哔声作为应答类似星球大战的bd1, 同时支持文字转语音, 只有基础功能, 运行逻辑没实现
   - 关键词唤醒 然后对话
6. [ ] 接入llm然后通过提示词让llm控制身体, 区分在线和离线
   - 目前离线模型推理很慢, 离线时只做简单表情处理, 在线模型才支持肢体动作
   - 接入llm后支持常见对话
   - 暴露给llm的功能: 肢体控制, 发声, 支持一些定时任务(比如: 日程, 代办, 备忘录)

## 长期计划
- 在添加完基本功能后希望机器人能够自主移动, 通过识别aruco码来让机器人回到充电桩.

## 使用方法
- 拿到对应的ele_bot程序后直接运行会在`hugging face`的默认目录下载模型, 然后程序退出手动调用`convert_all_models.py`进行模型转换再次启动程序即可.

### 编译

#### 本地编译 (macOS / Linux / Windows)
- ~~当运行在pc平台是, 使用的推理框架是`onnx`, 这时需要手动安装运行时.~~
   ```shell
   brew install onnxruntime # mac
   sudo apt install libonnxruntime-dev # ubuntu
   winget install Microsoft.OnnxRuntime # Windows
   ```
- 常规直接`cargo run --release`即可

#### 交叉编译到 RK3566 (推荐)

依赖: Docker Desktop / docker daemon, [cross-rs](https://github.com/cross-rs/cross) (`cargo install cross --git https://github.com/cross-rs/cross`).

```shell
# 编译并部署主程序 (默认 release)
./scripts/deploy_rk3566.sh

# 单独编译 / 单独部署
./scripts/deploy_rk3566.sh build
./scripts/deploy_rk3566.sh deploy

# 编译并部署 test_bd1 (BD1 声音测试 binary)
./scripts/deploy_rk3566.sh test_bd1

# debug 编译 (dev profile, 含调试符号)
./scripts/deploy_rk3566.sh --debug
./scripts/deploy_rk3566.sh build --debug

# 任意位置参数都能加 --debug, 同时支持 PROFILE 环境变量:
PROFILE=release-with-debug ./scripts/deploy_rk3566.sh
```

> **dev profile 警告**: RK3566 跨编译 `gemm-f16` 在无优化时汇编失败. debug 编译目前走不通, 但 release 正常. 调试建议用 release 包 + remote gdb.

环境变量 (覆盖默认值):

| 变量 | 默认值 | 说明 |
|---|---|---|
| `RK_DEVICE` | `radxa@192.168.2.159` | 目标 SSH 地址 |
| `RK_REMOTE_DIR` | `~/ElectronBotCli` | 目标路径 |
| `RK_PASSWORD` | `radxa` | sshpass 密码, **仅在 SSH 密钥失败时使用** |
| `HTTP_PROXY` / `HTTPS_PROXY` | `http://192.168.2.147:7890` | apt/cargo 走代理时设置 |
| `PROFILE` | `release` | `dev` / `release` / `release-with-debug`, 也可用 `--debug` 标志 |

示例:
```shell
# 换设备 + 换代理
RK_DEVICE=radxa@192.168.2.202 \
HTTP_PROXY=http://other-proxy:7890 \
./scripts/deploy_rk3566.sh

# 推荐先用 ssh 密钥免密码登录
ssh-copy-id radxa@192.168.2.159
./scripts/deploy_rk3566.sh deploy  # 之后自动走密钥
```

脚本内置:
- docker daemon / cross 二进制 / 磁盘空间 (≥8GB) 三项前置检查
- scp 后 sha256 校验, 传输截断直接报错
- Cargo 缓存挂载进容器, 增量编译命中 (实测 4 分 → 51 秒)

升级 cross-rs base image:
```shell
docker pull --platform linux/amd64 ghcr.io/cross-rs/aarch64-unknown-linux-gnu:edge
docker inspect --format='{{index .RepoDigests 0}}' ghcr.io/cross-rs/aarch64-unknown-linux-gnu:edge
# 把输出的 digest 替换 Dockerfile.cross 里的 FROM 行
```

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
# 推荐: 一键交叉编译+部署
./scripts/deploy_rk3566.sh

# 手动 (不推荐, 容易踩到库依赖问题)
cross build --target aarch64-unknown-linux-gnu --release -p ele_bot_server --bin ele_bot_server
scp target/aarch64-unknown-linux-gnu/release/ele_bot_server radxa@192.168.2.159:~/ElectronBotCli/
scp target/aarch64-unknown-linux-gnu/release/libsherpa-onnx-c-api.so target/aarch64-unknown-linux-gnu/release/libonnxruntime.so radxa@192.168.2.159:~/
scp assets/tools/convert_all_models.py  radxa@192.168.2.159:~/ElectronBotCli
```

### 待优化

1. [ ] 优化人脸识别方面的, 减少内存拷贝并持久化一些变量, 免得重复开辟大量空间.
2. [x] 在rk3566情况下使用硬件rga进行旋转 缩放 格式转换.

## 备注