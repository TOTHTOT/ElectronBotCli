import serial
import struct
import time

def run_test():
    port = '/dev/ttyACM0'
    baud = 115200

    try:
        ser = serial.Serial(port, baud, timeout=0.1)
        ser.dtr = True
        ser.rts = True
        time.sleep(0.5)
        ser.rts = False
        print(f"✅ 已打开串口 {port}，开始循环唤醒...")

        # 准备一个标准的 224 字节空包 (使能位=1, 目标角度全0)
        heartbeat = bytearray(224)
        heartbeat[0] = 0 # isEnabled = True

        count = 0
        while True:
            # 1. 持续向机器人发送 224 字节指令包
            ser.write(heartbeat)

            # 2. 检查是否有数据返回
            if ser.in_waiting >= 32:
                raw_data = ser.read(32)
                # 解析机器人发回的 32 字节包
                # 字节 1-25 是 6 个 float 角度
                try:
                    # 使用 struct 解析 6 个 float (每个4字节)
                    # '<' 代表小端模式 (STM32 通常是小端)
                    print(f"raw_data: {['%d' % a for a in raw_data]}\n")
                    angles = struct.unpack('<ffffff', raw_data[1:25])
                    print(f"\r[收] 角度数据: {['%.2f' % a for a in angles]} | 包序: {count}", end="")
                except Exception as parse_err:
                    print(f"\n解析失败: {raw_data.hex()}")

                count += 1

            # 3. 稍微停顿，不要把串口堵死（机器人代码里有 HAL_Delay(1)）
            time.sleep(0.01)

            # 每发 50 次包提示一下
            if count % 50 == 0 and count > 0:
                print(f"\n[提示] 已稳定通信，正在持续接收...")

    except KeyboardInterrupt:
        print("\n用户手动停止")
    except Exception as e:
        print(f"\n发生错误: {e}")
    finally:
        if 'ser' in locals():
            ser.close()

if __name__ == "__main__":
    run_test()