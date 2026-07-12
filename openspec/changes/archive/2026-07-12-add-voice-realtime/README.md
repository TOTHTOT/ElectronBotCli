# add-voice-realtime

修复音量不刷新 + 麦克风热切换不实时: 加 ServerEvent::Volume 协议 + 50ms 周期广播, 修 VoiceManager cancellation flag 让 rebuild 时旧 ASR 线程能立刻退出
