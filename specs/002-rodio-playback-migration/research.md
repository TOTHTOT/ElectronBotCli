# Phase 0 Research: rodio 0.22 API 核实与迁移映射

## Decision: rodio 0.22.2, playback-only feature

**Rationale**: rodio 是 cpal 生态的标准高层播放库, mixer 层对每个输入源
统一包 `UniformSourceIterator` (`mixer.rs:62`), 自动完成声道数与采样率
转换, 样本格式转换在 sink 写入设备时处理 — 正好覆盖 spec FR-001/FR-002。
`default-features = false, features = ["playback"]` 不引入 symphonia 等
解码器 (feature 清单已通过 `cargo info rodio` 核实)。

**Alternatives considered**:

- **继续手写 cpal 双格式回调** (现状): 已修复 PCM2912A 但只解决位宽,
  采样率不匹配仍会失败; 设备特判逻辑 ~200 行, 违反 spec FR-005。
- **cpal + rubato 重采样**: 只补采样率, 格式/声道转换仍需手写, 等于
  自己重造 rodio 的 mixer, 无收益。
- **symphonia**: 解码库, 不解决播放侧问题。

## Decision: `DeviceSinkBuilder::open_sink_or_fallback()` 替代手写格式探测

**Rationale**: `stream.rs:390` — 先按设备默认配置开流, 失败后遍历
`supported_output_configs` 逐个尝试, 全败才返回原始错误。这正是我们
`detect_sample_format` + `build_stream_with_fallback` 的通用版, 且每次
尝试都是真实开流 (不受 cpal 配置虚报影响 — PCM2912A 上 F32 虚报问题
自动免疫)。

**验证状态**: API 签名已从 vendored 源码核实 (`stream.rs:380-403`)。

## Decision: 播放完成语义映射

- **非流式** (`play`): `Player::append(SamplesBuffer)` +
  `player.sleep_until_end()` (`player.rs:313`)。`buffer::SamplesBuffer::new`
  接受 owned 数据 (`buffer.rs:40`), 不需要 `&'static` (避免了
  `StaticSamplesBuffer` 要求的切片泄漏)。
- **流式** (`start_streaming`): `queue::queue(true)` 得
  `(Arc<SourcesQueueInput>, SourcesQueueOutput)`, 后者 append 进 Player。
  `keep_alive_if_empty=true` 保证合成速度慢于播放速度 (欠载) 时队列源
  不提前结束; `mark_synthesis_done` 时调
  `set_keep_alive_if_empty(false)`, 队列播空后 `player.empty()` 为真,
  `is_done = synthesis_done && player.empty()`。
- 备选 `append_with_signal` (`queue.rs:77`, 返回 `Receiver<()>`) 也可做
  完成信号, 但需要在协议里多挂一个 channel, 不如 keep-alive 翻转直观。

**Rationale**: 与 spec FR-004 逐条对应, `VoiceManager` 调用模式
(write_chunk → mark_synthesis_done → 轮询 is_done) 零改动。

## Decision: 设备选择与初始化时机

**Rationale**: 保留 `voice/mod.rs::find_output_device` (按 cpal DeviceId/
name 选择, 含回退), 把选出的 `cpal::Device` 交给
`DeviceSinkBuilder::from_device`。流在 `TtsPlayer::new` 时打开并常驻 —
初始化失败即报错 (fail-fast), 不再每次播放开关流。

**类型备注**: rodio 0.22 的 `SampleRate = NonZero<u32>`,
`ChannelCount = NonZero<u16>` (`common.rs:5,8`), 构造时
`NonZeroU32::new(rate).expect("sample_rate 非零")`。

## 遗留风险 (实现期验证)

- `open_sink_or_fallback` 在 PCM2912A 上最终选中的配置需实机确认
  (预期 S16_LE/48kHz 立体声, mixer 重采样 16k→48k)。
- 流常驻对设备独占的影响: lckfb 部署目标只有本服务使用该声卡, 可接受;
  如未来出现竞争, `MixerDeviceSink` 支持 drop 后重建。
- `OwnedOutputStream` 是否有 tts.rs 以外的使用方, 实现前先 grep。
