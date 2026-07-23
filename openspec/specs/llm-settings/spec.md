# llm-settings Specification

## Purpose

设置页暴露 LLM 三项配置 (api_base / api_key / model), 用现有文本编辑模式
编辑, 走 `ClientMessage::SetConfig` 提交并由服务端持久化到 `config.toml`. 协议层
字段 (`AppConfig.llm_api_base` / `llm_api_key` / `llm_model`) 已存在, 此 spec 仅约束
客户端展示 + 客户端提交路径 + 服务端对空配置的兜底行为.
## Requirements
### Requirement: 设置页显示 LLM 三项配置

客户端设置页 SHALL 在设置列表中暴露以下 3 个文本字段, 占 3 个连续行, 顺序为:
"LLM API 地址", "LLM API Key", "LLM 模型". 这 3 行 SHALL 紧接在 Wifi 密码行之后,
并在设备项 (麦克风 / 扬声器) 行之前.

每行 SHALL:
- 显示当前 `AppConfig` 中对应字段的值; `llm_api_key` 即使非空也按明文显示
  (掩码处理**不**在本 spec 范围内)
- 当值为空字符串时, 显示为占位色 (暗灰色) `<未配置>`, 跟现有 "Wifi 密码" 空值风格一致
- Enter SHALL 进入文本编辑模式, 与现有 "Wifi 密码" 行使用相同的 `EditField` 路径
- Esc 退出编辑 SHALL 撤销修改 (恢复原值), 行为与现有 "Wifi 密码" 完全一致

索引常量 SHALL 在客户端代码中显式定义 (沿用 `SETTINGS_IDX_*` 命名约定), 不依赖裸数字.

#### Scenario: 默认空值显示
- **WHEN** `AppConfig::default()` 中 `llm_api_base` / `llm_api_key` 为 `""`, `llm_model` 为非空默认 `"doubao-seed-1-6-251015"`
- **THEN** 设置列表 SHALL 显示前两行为占位色 `<未配置>`, 第三行显示模型名默认值的正常色
- **AND** 设备项 (麦克风/扬声器) 行 SHALL 仍能正常显示并进入 picker

#### Scenario: 用户编辑后清空
- **WHEN** 用户进入 "LLM API 地址" 编辑, 删除全部字符后按 Enter 提交
- **THEN** `self.config.llm_api_base` SHALL 被设置为 `""`
- **AND** 客户端 SHALL 立即通过 `ClientMessage::SetConfig` 把更新后的 `AppConfig` 发给服务端
- **AND** 该行 SHALL 立即在 UI 上呈现占位色 `<未配置>`

### Requirement: LLM 设置走 SetConfig 提交

按 Enter 提交编辑 SHALL 把 buffer 写回 `self.config` 对应 LLM 字段, 并立即通过现有
`App::set_config` 调用发出 `ClientMessage::SetConfig`. 服务端 SHALL 在收到后写入
`config.toml` 并在后续 LLM 调用中使用新值.

提交路径 SHALL 与现有 "Wifi 名称" / "Wifi 密码" 行**完全一致**: 同样的 `commit_settings_edit`
match 分支结构, 同样的 `set_config(self.config.clone())` 调用顺序, 不引入新的网络消息
类型.

#### Scenario: 编辑 LLM 模型并提交
- **WHEN** 用户进入 "LLM 模型" 行编辑, 把 buffer 改成 `"gpt-4o"`, 按 Enter
- **THEN** `self.config.llm_model` SHALL 等于 `"gpt-4o"`
- **AND** 客户端 SHALL 发送 `ClientMessage::SetConfig { config }`, 消息中的 `llm_model` 字段 SHALL 是 `"gpt-4o"`
- **AND** 服务端 SHALL 把这个 `AppConfig` 持久化到 `config.toml`

#### Scenario: 编辑后 Esc 取消
- **WHEN** 用户编辑 "LLM API Key" 中途按 Esc
- **THEN** `self.config.llm_api_key` SHALL **不**被修改 (保留进入编辑前的值)
- **AND** 没有 `ClientMessage::SetConfig` 被发送

### Requirement: LLM 字段在首次进入时不强制非空

服务端 `LLM` 模块 SHALL 在 `llm_api_base == ""` 或 `llm_api_key == ""` 时继续运行 (允许空配置),
不 panic 不重复构造 `LlmManager`. 这意味着用户在 UI 中即使没填三项, 也能提交保存而不需要
回到手动 `config.toml` 删除旧字段.

#### Scenario: 服务端处理空 LLM Key
- **WHEN** 客户端发送 `SetConfig { config: AppConfig { llm_api_key: "", .. } }`
- **THEN** 服务端 SHALL 接受这个 `AppConfig` 并写入 `config.toml`, 不返回 `ServerEvent::Error`
- **AND** 后续 LLM 调用如果 `llm_api_key == ""`, SHALL 走现有的 "无 key 调用" 兜底逻辑 (失败由 LLM 服务端报错, 本 spec 不展开)
