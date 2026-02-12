# ElectronBotCli

- 这是[ElectronBot](https://github.com/peng-zhihui/ElectronBot.git)的命令行上位机, 使用rust编写, 支持跨平台运行.

- 现计划是通过rk3566(2G+16G)的核心板(55mm*40mm)+外扩底板以机器人背包的形式挂载, 将原先的usb尾插小板功能移植到底板.

## 包含功能

1. [x] 移植原上位机功能, 包含: usb的cdc通信, 舵机控制, 屏幕控制, 目前屏幕刷新能做到20fps.
2. [x] 支持表情显示, 采用[RoboEyes](https://github.com/FluxGarage/RoboEyes/tree/main)的实现方式, 动态生成表情.
3. [ ] 使用vosk调用usb麦克风实现语音唤醒.
4. [ ] 人脸表情识别并将结果转为机器人表情.
5. [ ] 人脸位置识别, 控制身体旋转保持人脸居中.
6. [ ] 语音对话, 预计采用哔哔声作为应答, 同时支持文字转语音.
7. [ ] 接入llm然后通过mcp让llm控制身体.
 
## 长期计划
- 在添加完基本功能后希望机器人能够自主移动, 通过识别aruco码来让机器人回到充电桩.

## 备注
1. 使用了`vosk`需要根据系统添加对应的动态库, 放在执行文件同一级目录, 比如:liberos.dll.