# 🛠 Radxa CM3 (RK3566) USB 硬件与开发环境配置手册
## 一、 硬件层：开启 USB 主机权限
RK3566 的 USB 接口默认可能处于从机（Peripheral）或禁用状态，需通过以下步骤强制开启供电与数据通道。

1. 系统 Overlay 配置：
   - 执行 sudo rsetup。 
   - 进入 Overlays -> Manage overlays。 
   - 勾选 [*] Set OTG port to Host mode。 
   - 确认 不勾选 Set OTG port to Peripheral mode。 
   - 保存并 重启系统。