# Feature Specification: LLM 模块接入 ZeroClaw 托管对话与记忆

**Feature Branch**: `001-zeroclaw-llm-integration`

**Created**: 2026-08-02

**Status**: Draft

**Input**: User description: "把 llm 模块接入 zeroclaw，由 zeroclaw 托管对话记录与个人信息（记忆），替代本仓库自行管理的对话历史"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - 多轮语音对话上下文自动延续 (Priority: P1)

用户与机器人进行多轮语音对话（例如"今天天气怎么样" → "那明天呢"），第二轮及之后的对话能自动携带前文的上下文，机器人给出语义连贯的回答。整个过程用户和本仓库代码都不需要手动维护、拼接或传递对话历史。

**Why this priority**: 这是接入 ZeroClaw 的核心动机——把对话历史管理从本仓库剥离。没有它，其它故事都没有意义。

**Independent Test**: 设备端启动完整语音链路（ASR → LLM → TTS），连续进行 3 轮以上有指代关系的对话，验证后续回答能正确引用前文；同时确认本仓库不再保存/传递任何对话历史。

**Acceptance Scenarios**:

1. **Given** 机器人正常运行且 ZeroClaw 服务可用，**When** 用户先问"我叫小明"，隔几轮后问"我叫什么名字"，**Then** 机器人回答中包含"小明"
2. **Given** 已完成一轮对话，**When** 用户用"它/那个/明天呢"等指代词继续追问，**Then** 机器人能基于上一轮主题作答而非答非所问
3. **Given** 对话进行中，**When** 开发者检查本仓库运行状态，**Then** 本仓库进程内不存在自行累积的对话历史副本（历史由 ZeroClaw 侧持有）

---

### User Story 2 - 用户个人信息长期记忆 (Priority: P2)

用户在对话中透露的个人信息（姓名、偏好、习惯等）被长期记住：即使机器人重启、隔天再对话，机器人仍能记起用户之前告诉它的信息，无需用户重复说明。

**Why this priority**: "个人信息不用自己管"是用户明确提出的一半诉求；但短期上下文的连续性（P1）是更基础的体验。

**Independent Test**: 告诉机器人一条个人信息（如"我喜欢听周杰伦"），重启机器人服务后再问"我喜欢听谁的歌"，验证回答正确。

**Acceptance Scenarios**:

1. **Given** 用户曾告知个人信息且机器人已重启过，**When** 用户询问该信息，**Then** 机器人能正确回忆
2. **Given** 用户选择"清空全部历史与记忆"，**When** 再次询问相关个人信息，**Then** 机器人不再引用任何已清空的信息（本期只提供整体清空入口，不支持对话内单条删除）

---

### User Story 3 - ZeroClaw 不可用时的行为 (Priority: P3)

当 ZeroClaw 服务未启动、崩溃或响应超时时，机器人以可预期的方式降级：不卡死、不静默丢失用户输入，而是给出明确反馈或回退。

**Why this priority**: 健壮性需求，重要但不阻塞核心链路；可以最后做。

**Independent Test**: 手动停止 ZeroClaw 服务后进行一轮语音对话，观察机器人行为。

**Acceptance Scenarios**:

1. **Given** ZeroClaw 服务不可用，**When** 用户发起对话，**Then** 机器人在合理时间内（≤5 秒）通过语音播报"服务不可用"类提示（不回退到现有 LLM 直连）；ZeroClaw 恢复后对话自动继续且记忆可访问
2. **Given** ZeroClaw 从不可用恢复，**When** 用户再次对话，**Then** 对话功能自动恢复且能访问到之前的记忆

---

### Edge Cases

- ZeroClaw 响应超时（如网络 LLM 服务缓慢）时，语音链路如何限时与反馈？
- 对话历史/记忆无限增长，是否有容量上限或清理策略？
- 多设备/多用户场景下记忆是否串扰（本期默认单用户单设备）？
- ZeroClaw 侧需要调用在线 LLM，设备断网时记忆可读但无法生成回复，如何表现？

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: 语音对话的 LLM 回复必须改由 ZeroClaw 生成，本仓库将识别后的用户文本转发给 ZeroClaw 并取回文本回复
- **FR-002**: 对话历史的存储、拼接与截断必须由 ZeroClaw 负责，本仓库 MUST NOT 再自行累积或传递对话历史（现有 session 历史管理代码应移除或停用）
- **FR-003**: 用户个人信息（姓名、偏好等）必须由 ZeroClaw 的记忆能力持久化，机器人重启后仍可被对话引用
- **FR-004**: 本仓库与 ZeroClaw 的交互必须支持超时与错误处理，任何 ZeroClaw 故障不得导致语音主链路崩溃或永久阻塞
- **FR-005**: 现有 LLM 配置项（API 地址、Key、模型）需要能传递给 ZeroClaw 使用，不要求用户维护两份配置；情感/动作分析（analyze_mood，驱动表情与舵机动作）**保留现有独立 LLM 调用链路**，不迁移到 ZeroClaw
- **FR-006**: 用户必须能清空/重置全部对话历史与个人记忆（隐私出口）

### Key Entities

- **对话会话 (Conversation)**: 一次持续的人机交互上下文；由 ZeroClaw 持有，本仓库只持有当前轮次的输入/输出
- **用户记忆 (Memory)**: ZeroClaw 持久化的用户个人信息条目；可跨重启、跨会话被对话引用
- **LLM 回复 (LlmResponse)**: 文本回复（走 TTS）；若情感/动作分析迁移，还包含情绪与动作指令

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 3 轮以上含指代词的连续对话中，上下文引用正确率 ≥ 90%（人工评测 10 组对话）
- **SC-002**: 重启机器人后，先前告知的个人信息回忆成功率 100%（评测 5 条不同类型信息）
- **SC-003**: 本仓库代码中不再存在对话历史的累积与传递逻辑（相关自管理代码移除，代码审查确认）
- **SC-004**: 单轮对话（ASR 结束 → TTS 开始播报）端到端延迟相比接入前劣化不超过 20%
- **SC-005**: ZeroClaw 进程被杀死后，语音主链路无 panic/卡死，100% 在 5 秒内给出降级反馈

## Assumptions

- ZeroClaw 作为独立服务/进程部署在 RK3566 设备端，本仓库通过其提供的本地交互接口（CLI/RPC/HTTP 之一，规划阶段确定）调用
- ZeroClaw 具备对话历史托管与用户记忆（memory）能力——这是本特性的前提，可行性在规划阶段验证（已有 target/zeroclaw-spike 初步探索）
- 设备端保持联网，ZeroClaw 底层调用与当前相同的在线 LLM（火山方舟 doubao）；本地 GGUF 小模型链路是否保留由降级策略决定
- 单用户单设备场景，不考虑多用户隔离
- 情感/动作分析（analyze_mood）保留现有链路，不在本特性范围内改动
