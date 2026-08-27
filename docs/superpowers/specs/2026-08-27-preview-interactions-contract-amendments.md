# Preview 交互系统契约补丁（Track A）

日期：2026-08-27
状态：待评审（spec amendment 提案）
范围：对 [2026-08-26 Preview 多端交互系统设计](2026-08-26-preview-multiplatform-interactions-design.md)（下称"原 spec"）的向后兼容契约补充；关联文档中心 [视觉上限提升计划](https://github.com/ZSeven-W/openpencil-docs) `openpencil/generation/2026-08-27-visual-ceiling-uplift-plan.md` Track A

## 1. 背景与窗口

视觉上限计划要求特效基底（shader fill / 效果节点 / Lottie / 滚动叙事）作为文档能力长进交互运行时，而不是事后打补丁。原 spec 处于"已确认设计，待实施"状态，跨端 trace 契约（§13.2）尚未冻结，是向后兼容纳入以下三项契约的最后窗口：

1. 可动画属性开放注册表（替代 §6.2 硬编码清单）；
2. 滚动原语（`$scroll` 绑定命名空间 + sticky/pin）；
3. 连续动画失效契约与失效分类对齐（§7.2 与现有代码不一致）。

三项均不改动原 spec 的任何既有语义，只做新增与文本勘误。

## 2. 现状锚点（代码核对结果，2026-08-27）

以下事实来自现场代码，补丁的落点据此确定：

- 事件 schema 已含 `on_scroll`（`vendor/jian/crates/jian-ops-schema/src/events.rs:176`）、`on_press_start/end/cancel`、`on_swipe`、`on_context_menu`（同文件 :144-152）——原 spec §5.2 所称"新增事件"在 schema 层已存在，缺的是运行时接通。
- `GestureOverrides` 已含 `double_tap_timeout/slop`、`swipe_min_distance/velocity`、`axis_lock`（`jian-ops-schema/src/gestures.rs:59-71`）——原 spec §4.2 所称"新增字段"同样已在 schema。
- **animate 动作不存在**：动作注册表（`vendor/jian/crates/jian-core/src/action/actions/mod.rs:22`）无 `animate`/`toggle`/`show`/`hide`/`scroll_to`/`dismiss_keyboard`；全库无 `Easing` 类型；Action body 在 schema 层故意不校验（`events.rs:85` 注释），未知 body 经 `ExtraJson` round-trip。
- 失效分类为**三分类** `InvalidationClass { PaintOnly, LayoutSpatial, Interactive }`（`jian-core/src/binding.rs:39-66`），与原 spec §7.2 的四分类（PaintOnly/HitTest/Relayout/Navigation）不一致，且无 Navigation。
- 转场期间输入当前**直接丢弃**（`crates/op-preview-core/src/input.rs:21-32`），与原 spec §7.3"保留一个安全离散输入并回放"存在行为差距。
- **属性表有两张，不是一张**（2026-08-28 补充核对）：`binding.rs:45` 的 `classify_binding` 把属性名映射到失效分类（LayoutSpatial/Interactive，其余默认 PaintOnly），而 `jian-core/src/render/scene.rs:415` 起另有一张 match 把属性名映射到**值如何写入渲染对象**（`opacity` 写 opacity 字段、`x/y/width/height` 写 rect overrides、`fill[0].color` 走 `set_first_fill_color`）。两张表的属性集并不一致——`opacity` 只在后者有专门分支、在前者落进默认分类；`gap`/`padding` 只在前者。
- 渲染侧已有 shader 通路：`ShaderSpec { sksl, uniforms, opacity, fallback }` 与 `DrawOp::ShaderRect`（`jian-core/src/render/paint.rs:200/262`），SkSL `RuntimeEffect` 编译缓存（`jian-skia/src/backend.rs:52`、`:828`）；无文档级 blendMode 属性。

## 3. 补丁一：可动画属性开放注册表

**替代**原 spec §6.2 的固定首期属性清单（8 个属性保留为注册表的首批内置条目，行为不变）。

### 3.1 契约

- 新增 `AnimatablePropertyRegistry`，由 jian-core 持有为唯一权威，条目含：`name`、`value_type`（number/color/length/angle）、`interpolate`（线性/离散/颜色空间）、`invalidation_class`（只允许 PaintOnly 或 HitTest，禁止 Relayout 属性进入连续动画）、`apply`（值如何写入渲染对象——收编 §2 所述 `scene.rs` 的第二张 match，否则会出现"注册表声明可动画、渲染侧不知道怎么写入"的裂缝）、`capability`（缺省时各端必须支持）。
- `animate` body 的 `property` 字段为字符串，**不在 schema 层枚举校验**（沿用 Action body 不校验的既有策略，`ExtraJson` 兼容未知值）；运行时查注册表，未注册属性产生结构化诊断 `UnknownAnimatableProperty`，不执行也不丢失文档数据。
- 注册表通过查询 API 暴露给 Interact Tab Animation Editor 与 AI/MCP——三者消费同一张表（符合原 spec §8.2 "AI/MCP 写入同一 schema，不拥有旁路格式"）。
- 预留 `shader.<uniform>` 命名空间：shader fill 节点（视觉上限计划 Track B，另行立项）落地后，其 uniform 参数经注册表成为可动画属性，无需修改 animate 契约本身。

### 3.2 兼容性

纯新增：`animate` 动作与注册表均不存在于旧文档；旧文档 Golden 无行为变化。首期 8 个内置属性的语义与原 spec §6.2 文字完全一致。

### 3.3 验收

- 注册表单元测试：内置 8 属性完备、非法属性诊断、Relayout 属性注册被拒；
- Interact Tab 与 MCP 读取同一注册表（snapshot 测试）；
- 旧 `.op` Golden round-trip 无变化。

## 4. 补丁二：滚动原语

**新增**，不改变 `on_scroll` 既有事件语义（原 spec §5.1）。

### 4.1 契约

- 表达式/绑定上下文新增只读命名空间 `$scroll`：`offset`、`maxOffset`、`progress`（0..1，maxOffset 为 0 时定义为 0）、`direction`。取值来自最近的可滚动祖先；无滚动祖先时绑定求值为默认值并产生一次性诊断。
- `$scroll.*` 只允许绑定到 **PaintOnly 类**属性（opacity/fill/stroke/textColor/transform 系），绑定编译期强制——防止滚动驱动每帧 relayout（与原 spec §6.2 连续输入不得每帧 relayout 的约束一致）。
- 滚动节点新增可选属性 `stickyChildren` / 节点级 `pin`：pin 住的节点在祖先滚动时保持视口位置（等价 CSS sticky/fixed 的受控子集），参与命中测试重算（HitTest 失效），但不参与 relayout。
- 滚动驱动的绑定更新走统一 redraw deadline（原 spec §13.4），每帧最多一次重绘调度。

### 4.2 兼容性

纯新增字段与命名空间；旧文档无 `$scroll` 引用，行为不变。

### 4.3 验收

- 绑定编译期拒绝 `$scroll` → width/height/x/y 的用例（结构化诊断）；
- 跨端 trace：同一滚动输入序列 → 相同 `$scroll.progress` 值与相同 state diff（纳入原 spec §13.2）；
- pin 节点在滚动中的 visual snapshot 与命中测试一致性测试。

## 5. 补丁三：连续动画失效契约与失效分类对齐

**对齐**原 spec §7.2 与现有代码的三分类，**新增**时间驱动动画的帧调度契约。

### 5.1 失效分类迁移映射

| 现有（`binding.rs:39-66`） | 原 spec §7.2 | 迁移 |
| --- | --- | --- |
| `PaintOnly` | PaintOnly | 直接对应 |
| `LayoutSpatial` | Relayout | 更名对应，语义不变 |
| `Interactive` | HitTest | 对应（可见性/命中区域） |
| —（缺失） | Navigation | 新增：路由栈或页面挂载变化 |

实施期在 `binding.rs` 完成更名与 Navigation 补充；`classify_binding` 的属性映射表同步进入注册表（补丁一），单一数据源。**`scene.rs` 的值应用 match 一并收编**——只并分类表会留下两套属性契约（一套管分类、一套管写入），两边属性集已经不一致（见 §2），继续分开只会越漂越远。

### 5.2 连续动画帧调度契约

- 时间驱动动画（`animate` 循环、shader 时间 uniform、Lottie 节点）一律产生 **PaintOnly** 帧更新，经统一 redraw deadline 调度（原 spec §13.4"不为每个节点创建独立 timer"）；
- 任何连续动画路径触发 relayout 视为缺陷，进 Debugger 诊断；
- 转场期间的动画行为遵循原 spec §7.3；同时勘误：现有 `op-preview-core/src/input.rs` 转场丢输入行为需在实施期补齐为"单槽离散输入回放"，补丁不改变该要求，仅登记差距。

### 5.3 验收

- 四分类 schema/Runtime/Trace 三层测试通过（原 spec §13.1/13.2）；
- 连续动画场景 60Hz 帧预算测量（按原 spec §13.4 单独报告，不进易抖动 CI 断言）；
- 动画中强制 relayout 的对抗用例产生诊断而非崩溃。

## 6. 原 spec 文本勘误（实施前修订）

1. §4.2/§5.2：将 `doubleTapTimeout` 等 5 个 GestureOverrides 字段与 `onPressStart/onSwipe/onContextMenu` 等事件从"新增"改述为"schema 已存在，需接通运行时"（锚点见 §2）；
2. §6.2：首期属性清单改述为"注册表首批内置条目"，清单内容不变；
3. §7.2：失效分类以本补丁 §5.1 映射为准；
4. §7.3：登记 `input.rs` 现状差距，实施任务中显式包含单槽回放。

## 7. 实施边界

- 本补丁只改契约与 spec 文本，不含任何运行时实现；
- 实现归原 spec 的实施计划（`docs/superpowers/plans/2026-08-26-preview-*.md`）吸收：补丁一进 runtime foundation 任务，补丁二进 binding/scroll 相关任务，补丁三进 trace 与性能验收任务；
- shader fill / Lottie 节点本体（视觉上限计划 Track B）不在本补丁范围，仅预留 `shader.<uniform>` 命名空间；
- 各端 Host Adapter 不得为注册表属性或 `$scroll` 引入平台私有语义（沿用原 spec §14 约束）。
