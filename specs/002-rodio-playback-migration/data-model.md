# Phase 1 Data Model: 播放层实体与状态

## 实体

### TtsAudio (不变)

| 字段 | 类型 | 说明 |
|------|------|------|
| samples | Vec<f32> | 单声道浮点样本, 合成引擎产出 |
| sample_rate | u32 | 源采样率 (当前 kokoro 模型 = 16000) |
| channels | u16 | 声道数 (当前 = 1) |

迁移后作为 `SamplesBuffer::new(channels, sample_rate, samples)` 的直接输入,
位宽/声道/采样率适配全部下放给 mixer。

### TtsPlayer (重写内部)

| 字段 | 类型 | 说明 |
|------|------|------|
| sink | rodio MixerDeviceSink | 打开的输出流 + mixer, 初始化时创建并常驻 |

不变量: `sink` 在 `TtsPlayer` 存活期有效; Drop 时释放设备句柄
(供 `SharedState::rebuild_voice` 的 RAII 释放链使用)。

### StreamPlayerHandle (公开 API 不变)

| 成员 | 迁移后实现 | 说明 |
|------|-----------|------|
| write_chunk(chunk, progress) | `queue_input.append(SamplesBuffer)` | 合成线程调用 |
| mark_synthesis_done() | 置标志 + `set_keep_alive_if_empty(false)` | 合成收尾 |
| is_done() | `synthesis_done && player.empty()` | 调用方轮询 |

内部状态: `player: Player`, `queue_input: Arc<SourcesQueueInput>`,
`synthesis_done: Arc<AtomicBool>`, `sample_rate` (构造 SamplesBuffer 用)。

## 状态机

### 流式播放 (一次 speak_streaming 调用)

```text
[空闲] --start_streaming--> [合成中+播放中]
   queue keep_alive=true, write_chunk 追加样本
[合成中+播放中] --mark_synthesis_done--> [收尾中]
   keep_alive=false, 队列播空后 player.empty()=true
[收尾中] --is_done()==true--> [空闲]
   handle drop, Player/queue 释放
```

### 整段播放 (play)

```text
[空闲] --play(audio)--> [播放中] --sleep_until_end 返回--> [空闲]
```

## 验证规则 (来自 spec)

- FR-003: 两条路径共用同一 `TtsPlayer.sink`, 行为一致。
- FR-004: 流式完成判定必须同时满足 `synthesis_done` 与 `player.empty()`,
  缺一不判完成 (防提前截断与死等)。
- 边界: 合成线程报错时 `mark_synthesis_done` 仍被调用 (现有
  `VoiceManager::speak_streaming` 结构保证), 流程正常收尾。
