import usb.core
import usb.util
import time
import struct

def run_test():
    # 1. 发现设备
    dev = usb.core.find(idVendor=0x1001, idProduct=0x8023)
    if dev is None:
        print("❌ 找不到 ElectronBot")
        return

    print(f"🔍 找到设备: {dev.idVendor:04x}:{dev.idProduct:04x}")

    try:
        # 2. 关键：重置设备，强制让它回到初始状态
        print("🔄 正在重置设备...")
        dev.reset()
        time.sleep(1) # 必须等一下，重置会导致设备短暂断开

        # 3. 寻找并声明接口
        # ElectronBot 可能是复合设备，尝试逐个接口声明
        success = False
        for i in [0, 1]:
            try:
                if dev.is_kernel_driver_active(i):
                    dev.detach_kernel_driver(i)
                usb.util.claim_interface(dev, i)
                print(f"✅ 成功声明接口 (Interface): {i}")
                success = True
                break
            except Exception as e:
                print(f"⚠️ 尝试接口 {i} 失败: {e}")

        if not success:
            print("❌ 无法声明任何接口，请尝试 sudo 运行")
            return

        # 4. 数据交互
        # 注意：如果 0x01 报错，请尝试 0x02，这取决于固件
        ep_out = 0x01 
        ep_in = 0x81
        
        heartbeat = bytearray(224)
        heartbeat[0] = 0 # Enable 

        print("🚀 开始发送数据...")
        count = 0
        while True:
            try:
                # 写入 224 字节
                dev.write(ep_out, heartbeat, timeout=1000)
                
                # 读取 32 字节返回包
                raw = dev.read(ep_in, 32, timeout=1000)
                if len(raw) >= 32:
                    # 解析角度 (6个float)
                    angles = struct.unpack('<ffffff', raw[1:25])
                    print(f"\r[收] 角度: {['%.2f' % a for a in angles]}", end="")
                    count += 1
            except usb.core.USBError as e:
                print(f"\n⚠️ 传输错误: {e}")
                break
            
            time.sleep(0.02)

    except Exception as e:
        print(f"\n❌ 运行错误: {e}")
    finally:
        usb.util.dispose_resources(dev)

if __name__ == "__main__":
    run_test()
