# Tasks: add-llm-settings

## 1. 数据结构 & 索引常量 (基础层)

- [x] 1.1 `crates/ele_bot_client/src/app/route.rs` 的 `EditField` 新增 `cursor: usize` 字段;
      改 `EditField::new` 接收 cursor 参数 (不默认 0); 加 char-level 工具方法:
      `insert_char` / `delete_back` / `delete_forward` / `move_cursor_left` / `move_cursor_right`
      / `move_cursor_to_start` / `move_cursor_to_end`. 内部统一用 `buffer.char_indices()` 做
      char ↔ byte 换算, 外部 API 只用 char 索引. 完成后补 `///` rustdoc (`/// 一句话职责 +
      /// 边界/不变量 (字符按 char 计数, 非 byte) + `# Examples` `rust,ignore``).
- [x] 1.2 `crates/ele_bot_client/src/app/mod.rs` 的 `SETTINGS_LABELS` 数组扩展到 7 项
      (在 Wifi 密码 idx 1 之后、麦克风 idx 2 之前插入 LLM 三项); 新增索引常量
      `SETTINGS_IDX_LLM_API_BASE / LLM_API_KEY / LLM_MODEL`; 重新命名既有常量
      `SETTINGS_IDX_SPEECH / OUTPUT` 不变值, 因为现有 match 分支用名字 — 注释里指明
      "原 idx 2/3 现在是 LLM 三项, 设备项顺延到 5/6".
- [x] 1.3 完成后跑 `cargo fmt --all && cargo check --all-features --all-targets`
      (EditField 是公开 API, 改签名要确认没有其它调用点 — 已知只有
      `app/mod.rs::begin_settings_edit` 一处直接 `EditField::new`, 下一步一起改).

## 2. App 路由分支

- [x] 2.1 `crates/ele_bot_client/src/app/mod.rs::begin_settings_edit` 的 match 增加 3 个
      LLM 分支 (`SETTINGS_IDX_LLM_API_BASE / LLM_API_KEY / LLM_MODEL`), 各从
      `self.config.llm_*` 取初值; 初始化 cursor = `buffer.chars().count()` (末尾, 方便追加).
      注释里说明"`_ => return` 兜底保留, 让麦克风/扬声器仍走 picker 不进编辑态".
- [x] 2.2 `crates/ele_bot_client/src/app/mod.rs::commit_settings_edit` 的 match 增加 3 个
      LLM 写回分支; `Esc` 取消路径 (`cancel_settings_edit`) 不动 — buffer 直接 clear,
      self.config.* 不变. 注释说明 "commit_settings_edit 走 `set_config`, 跟 wifi 完全一致".
- [x] 2.3 完成后跑 `cargo check --all-features --all-targets` 验证索引常量改名没有
      影响其它引用点 (已知引用: input/mod.rs, settings.rs 不会 hard-code 数字).

## 3. ViewModel 镜像

- [x] 3.1 `crates/ele_bot_client/src/ui/viewmodel/settings.rs` 的 `SettingsViewModel`
      新增 `edit_cursor: usize` 字段, 从 `Route::Settings.editing` 的 `EditField.cursor`
      镜像; 选中和编辑态分支都对齐 cursor 赋值.
- [x] 3.2 同文件 `from_app` 的 `items: Vec<SettingItem>` 构造, 在 idx 1 (Wifi 密码) 之后
      插入 LLM 三项, `label = "LLM API 地址" / "LLM API Key" / "LLM 模型"`,
      `value` 取 `app.config.llm_api_base / llm_api_key / llm_model` 各自的 clone.
      设备项保持原 idx 顺延到 5 / 6, 不动.

## 4. UI 渲染 (caret + 信息条)

- [x] 4.1 `crates/ele_bot_client/src/ui/pages/settings.rs::render_setting_item` 改三段渲染:
      `before = buffer.chars().take(cursor).collect()`, `after = buffer.chars().skip(cursor).collect()`,
      中间插一个 caret span (反色 `█` 块字符, 与前后段同 `bg(Color::White)` 区别于普通 fg);
      补 `///` rustdoc 解释 "三段渲染原因: 不用 `Frame::set_cursor_position` 因为
      `crate::ui::mod.rs::render` 的 popup layer 在 EditField 文本之上 Clear 会抹掉
      终端原生 cursor, 块字符方案对 overlay 渲染兼容".
- [x] 4.2 同文件 `render_info_bar` 改文案:
      编辑态 `"操作: [Enter] 保存  [Esc] 取消  [Backspace] 删前  [Delete] 删后  [←→] 移动  [Home/End] 跳首尾"`,
      非编辑态保持原 `"操作: [↑/↓] 选择  [Enter] 编辑/选设备  [Esc] 退出  [R] 刷新设备列表"`.
- [x] 4.3 验证空 buffer 时仍正确显示 caret (没有 `before`/`after`, 仅一个 caret span).

## 5. 输入分发扩展

- [x] 5.1 `crates/ele_bot_client/src/input/mod.rs` 新增模块内辅助
      `fn apply_edit_key(f: &mut EditField, code: KeyCode)`, 把按键统一映射到
      `EditField` 方法 (Left/Right/Home/End/Delete/Backspace/Char); 注释里明确 "未识别的
      KeyCode 原样不动, 不会修改 buffer/cursor, 跟现状兼容". 补 `///` rustdoc.
- [x] 5.2 现有 EditField overlay 分发 match 改用 `apply_edit_key(&mut f, code)`, Enter/Esc
      各自走 `commit_settings_edit` / `cancel_settings_edit` 不变. match 把兜底改成
      通用 `code => apply_edit_key(...)` 而非按键逐一支.
- [x] 5.3 检查 `Char(c)` 是否需要过滤控制键 (e.g. `Char('\t')`). crossterm 把 Tab 编成独立
      `KeyCode::Tab`, 不会走 Char 分支; 不需要额外过滤. 注释里写明依赖此前提.

## 6. 三件套 + 自检 + 提交

- [x] 6.1 跑 `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings &&
      cargo check --all-features --all-targets`, 必须全部 0 错 0 warn. clippy 报默认 lint
      默认可能抓到 `useless_vec` / `needless_return` 之类小问题, 改完再重跑.
- [ ] 6.2 自检 (按 spec 验收清单逐项) — **留作 pending**, 自动化环境跑不动 ratatui,
      把清单附进 commit message. 用户跑 dev 时逐项过:
      - 进设置页能看到 7 项, 前 2 项 Wifi, 中间 3 项 LLM, 最后 2 项设备
      - 进入 "LLM 模型" 编辑, 光标在末尾 (开 buffer 默认值时)
      - 按 Left → caret 左移 1 列; 按 Right → 还原
      - 按 Home → caret 到 0; 按 End → caret 到末尾
      - 中部按 Char('X') → buffer 在中插入 'X'
      - 按 Backspace / Delete 验证前后各删一个字符
      - 中文 buffer (进 "LLM API 地址" 删空, 试输入"豆包模型") 验证 char-level 操作不破坏 UTF-8
      - 改完 Enter 提交, 退到设置列表再点回 "LLM 模型", 看到刚保存的值
      - Esc 取消, buffer 回到进入前的值
- [x] 6.3 commit (`a6b14f7`), message:
      `新增/设置页 LLM 三项 + EditField 字符级光标与 caret 渲染`
      已 commit, 三件套干净 + 用户手测清单放进 commit body.
