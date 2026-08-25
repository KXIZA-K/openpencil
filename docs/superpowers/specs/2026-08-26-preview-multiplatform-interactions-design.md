# Preview 多端交互系统设计

日期：2026-08-26
状态：已确认设计，待实施计划
范围：OpenPencil Preview、Jian Runtime、Desktop/Web/iOS/Android/Harmony 宿主及编辑器 Interact Tab

## 1. 背景与目标

OpenPencil Preview 已具备单点指针、点击、Hover、滚轮、键盘输入、基础表单状态、App Mode 页面导航和简单路由转场，但现有能力存在三类断层：

1. Schema 已声明 DoubleTap、LongPress、Pan、Scale、Rotate、输入与滚动事件，Preview 实际执行面仍主要是 mouse-like 输入。
2. Runtime 状态覆盖集中在表单值和文本内容，显隐、视觉样式、几何、组件状态与动画尚未形成完整闭环。
3. Desktop、Web 和移动客户端的输入采集、IME、系统副作用与能力降级没有统一契约，容易出现单端可演示、多端行为漂移。

本项目的目标是建立一套共享 Preview 交互运行时：既能完成可用的 App 原型交互，也能承载 Hover、按压、拖动、手势、动画和转场等微交互；所有端使用相同事件、动作和状态语义，客户端能力是一等公民。

## 2. 已确认的产品决策

- 同时建设 App 原型交互和微交互，不把其中一类推迟到后续版本。
- Desktop、Web、iOS、Android、Harmony 必须消费同一运行时语义，客户端优先接入与验收。
- 在现有 `events`、`bindings`、`gestures` 和 lifecycle schema 上向后兼容扩展，不引入破坏性的 Interaction v2。
- 扩展现有属性面板 Interact Tab，复杂交互不只依赖手写 JSON、AI 或 MCP。
- 采用“统一语义、按平台能力适配”：触摸端以按压/聚焦反馈替代 Hover 视觉，右键映射长按；不支持的能力必须给出诊断。
- 系统副作用采用受控白名单：Open URL、Copy、Share、Haptic、Focus/Keyboard。禁止任意脚本、任意网络请求和系统命令。
- 核心实现进入 Jian 共享运行时；`op-preview-core` 不复制第二套动作解释器。

## 3. 非目标

本项目不包含：

- 任意 JavaScript、QuickJS 或用户自定义原生代码执行。
- 在 Preview 作者 UI 中暴露 `fetch`、WebSocket、任意 `call` 或系统命令。
- 完整的可视化状态机连线画布；首期使用结构化 Interact Tab。
- 物理引擎、3D 手势、音视频时间线或复杂粒子系统。
- 把 Preview 的临时运行状态写回设计文档。
- 多选节点上的事件广播式批量覆盖。
- 将某一端的专属行为视为共享语义；平台能力只能通过 Adapter 和 capability 描述进入。

## 4. 总体架构

```text
Platform InputAdapter
  -> PreviewInput
  -> Jian Gesture Arena
  -> SemanticEvent / EventHandlers
  -> Shared ActionExecutor
       -> Runtime state / bindings
       -> Router / overlays / scroll / animation
       -> PreviewEffect queue
            -> Platform EffectAdapter
            -> PreviewEffectResult
```

### 4.1 Platform InputAdapter

每个平台只负责收集事实，不解释业务动作。输入统一为 `PreviewInput`：

- Pointer：稳定 pointer id、Mouse/Touch/Pen 类型、Down/Move/Up/Cancel、逻辑坐标、压力、按钮、修饰键、单调时间。
- Wheel/Trackpad：二维 delta、单位与惯性阶段。
- Keyboard：key、code、repeat、修饰键。
- IME：Preedit、Commit、Cancel、UTF-8 选择范围。
- Lifecycle：Foreground、Background、Terminate。

宿主必须在进入 Preview 时声明 `PreviewHostCapabilities`，包括 Hover、ContextMenu、MultiTouch、Haptic、Share、Clipboard、OpenUrl、HardwareKeyboard、ImeComposition 等能力。

### 4.2 Jian Gesture Arena

手势识别与竞争统一放在 Jian，不在每个宿主重复：

- Pan 超过阈值后抢占 Tap。
- LongPress 到时后抢占 Tap。
- 同时配置 Tap 与 DoubleTap 时，单击等待双击窗口；只配置 Tap 时立即触发。
- `onPressStart` 在主指针 Down 时触发；Pan/Scale/Rotate 抢占或系统 Cancel 时触发 `onPressCancel`；未被取消的 Up 触发 `onPressEnd`。
- Swipe 与 Pan 竞争：节点声明任何 Pan handler 时由 Pan 识别，否则由 Swipe 识别，单次输入序列只产生一种终端拖动语义。
- Scale 与 Rotate 可在同一双指会话中协同识别。
- ContextMenu 在桌面由右键触发；触摸端有显式 `onLongPress` 时优先触发 LongPress，否则 LongPress 手势回退为 `onContextMenu`，禁止同一次长按执行两组动作。
- Touch 不执行任意 Hover Action，只产生运行时按压/聚焦视觉反馈。
- 节点 `gestures.disabled`、`dragThreshold`、`longPressDuration` 和 `rawPointer` 继续生效。

新增的可选 GestureOverrides：

- `doubleTapTimeout`
- `doubleTapSlop`
- `swipeMinDistance`
- `swipeMinVelocity`
- `axisLock`

所有字段缺省时采用共享默认值，旧文档行为不变。

### 4.3 Shared ActionExecutor

Jian ActionRegistry 是唯一动作解释器。`op-preview-core` 只提供文档投影、宿主能力、状态覆盖和 Effect drain。

Preview 额外增加 `PreviewActionPolicy`。它按动作名做白名单检查，先于 CapabilityGate 运行。即使文档声明 Network capability，Preview 也不会执行 `fetch`、WebSocket 或任意 `call`。

### 4.4 PreviewEffect

系统副作用不直接在 Runtime 内调用平台 API，而是生成有序 Effect：

- `OpenUrl`
- `Copy`
- `Share`
- `Haptic`
- `FocusNode`
- `BlurFocus`
- `DismissKeyboard`
- `Toast`
- `Alert`
- `Confirm`

每个 Effect 带唯一 id、来源节点、来源事件、用户激活凭证和所需 capability。宿主回传 `Success`、`Cancelled`、`Unsupported` 或结构化错误。异步结果只能继续原 ActionList 中已声明的分支。

## 5. 事件目录

### 5.1 已有事件完整接通

- `onTap`
- `onDoubleTap`
- `onLongPress`
- `onPanStart`、`onPanUpdate`、`onPanEnd`
- `onScaleStart`、`onScaleUpdate`、`onScaleEnd`
- `onRotateStart`、`onRotateUpdate`、`onRotateEnd`
- `onHoverEnter`、`onHoverLeave`
- `onChange`、`onSubmit`、`onFocus`、`onBlur`、`onKey`
- `onScroll`、`onReachEnd`
- App、Page、Node 现有 lifecycle hooks

### 5.2 向后兼容新增事件

- `onPressStart`
- `onPressEnd`
- `onPressCancel`
- `onSwipe`
- `onContextMenu`

### 5.3 标准事件载荷

事件通过只读 `$event` 暴露一致载荷：

- Pointer/Tap：全局与局部坐标、pointer type、button、pressure、modifiers。
- Pan：本帧 delta、累计 translation、velocity、起点与当前位置。
- Swipe：direction、distance、velocity。
- Scale：scale、deltaScale、focal point。
- Rotate：rotation、deltaRotation、focal point。
- Key：key、code、repeat、modifiers。
- Change/Submit：value、checked、selectedValue 等控件语义值。
- Scroll：offset、delta、maxOffset、direction。
- Lifecycle：previous/next route、foreground/background 原因等可证明信息。

## 6. 动作目录

### 6.1 Interact Tab 可配置动作

状态：

- `set`
- `toggle`
- `delete`
- `reset`

控制流：

- 顺序执行
- `if`
- `delay`
- `parallel`

导航：

- `push`
- `replace`
- `pop`
- navigation `reset`

UI：

- `show`
- `hide`
- `toggle_visibility`
- `focus`
- `blur`
- `scroll_to`
- `animate`

反馈：

- `toast`
- `alert`
- `confirm`

系统白名单：

- `open_url`
- `copy`
- `share`
- `haptic`
- `dismiss_keyboard`

### 6.2 Animate 动作

`animate` 的 body 使用结构化字段：

- target，缺省为 `$self`
- from，可选；缺省读取当前 runtime 值
- to
- durationMs
- delayMs
- easing
- iterations
- direction
- fillMode

首期可动画属性：opacity、x、y、rotation、scaleX、scaleY、fill、stroke 和 cornerRadius。Width/Height 的离散状态切换允许触发布局动画，但连续手势不得每帧启动完整文档 relayout。

### 6.3 未暴露动作

`fetch`、WebSocket、storage wipe、notify、任意 `call` 和未知脚本不会出现在 Interact Tab。历史文件中的这些动作原样保留，但 Preview 返回 `ActionRejected::Policy` 诊断，不静默执行或删除。

## 7. Runtime 状态、绑定与失效分类

### 7.1 绑定覆盖范围

- content、value、checked、selectedValue
- visible
- opacity、fill、stroke、textColor
- x、y、width、height
- rotation、scaleX、scaleY
- component variant / active state

### 7.2 失效分类

- PaintOnly：颜色、透明度、无需布局的 transform。
- HitTest：可见性、transform 或命中区域变化。
- Relayout：内容、宽高、布局位置、variant 结构变化。
- Navigation：路由栈或页面挂载变化。

任何影响几何的状态变化都必须重建命中测试，禁止画面与交互区域漂移。PaintOnly 更新不得触发完整 relayout。

### 7.3 转场期间输入

- 进入路由转场时取消当前连续手势与 pointer capture。
- Pan、Scale、Rotate、Wheel 等连续输入不排队。
- 最多保留一个安全的离散输入：Tap、Submit 或 Back；新输入替换旧输入。
- 转场结束后重新命中测试，只有目标仍存在且输入仍合法才回放。
- 系统副作用不得在排队阶段执行。

## 8. Interact Tab 作者体验

### 8.1 节点交互列表

单选节点时显示独立 Interact Tab：

- 每张交互卡片对应一个 trigger。
- 卡片内展示有序 ActionList。
- 支持新增、编辑、排序、启用/停用、复制和删除。
- 每次结构变化是一个独立 Undo 步骤。
- 多选只显示汇总和“清除共同交互”，不广播覆盖不同节点。

### 8.2 结构化编辑器

- Target Picker：从画布或图层树选择节点。
- Route Picker：选择已有 Screen/Route。
- Variable Picker：浏览 `$app`、`$page`、`$state`、`$self`、`$event`。
- Condition Builder：字段化条件，可切换到表达式输入。
- Animation Editor：属性、起止值、时长、延迟、缓动、循环、反向。
- Platform Badge：标明 Pointer-only、Touch fallback、System Effect 和缺失 capability。

未知事件、动作和 body 字段必须原样保留。旧编辑器可以显示“高级/未知动作”，但保存时不得丢失未来版本数据。

### 8.3 响应式布局

- Desktop、Web、iPad：侧栏。
- Phone：全高 Sheet。
- 所有尺寸使用同一 ActionBuilder、校验器和文档命令。
- AI/MCP 写入同一 schema，不拥有旁路格式。

## 9. Preview Debugger

Debug UI 默认关闭：

- Run、Pause、Reset runtime state。
- 当前 Screen、路由栈、焦点节点、pointer capture 和活跃手势。
- 有界事件流：trigger -> action -> state diff -> effect -> result。
- State Inspector：按作用域查看变量和最后写入来源。
- 日志节点可反查并高亮画布目标。
- Unsupported capability、无效表达式、未知 target、动作取消和策略拒绝使用结构化诊断。

事件日志使用固定容量 ring buffer，默认 256 条；超出后丢弃最旧记录。日志不得记录剪贴板正文、Share 私密正文或凭据。

Desktop/Web/iPad 使用侧栏；Phone 使用底部 Sheet。

## 10. 多端 Host Adapter

### 10.1 客户端优先顺序

1. iOS
2. Android
3. Harmony
4. Desktop Native
5. Web

共享 Runtime 与 Trace 契约先落地，Host Adapter 可以并行开发，但任何平台不得自定义动作语义。

### 10.2 iOS

- UITouch 多指 pointer id、压力与 Pencil 类型。
- Pinch/Rotate/LongPress 进入统一 PreviewInput。
- 软件键盘、硬件键盘与中文 IME 的 Preedit/Commit/Cancel。
- UIPasteboard、UIActivityViewController、UIImpactFeedbackGenerator、Open URL。

### 10.3 Android

- MotionEvent 多指、tool type、pressure 与 eventTime。
- InputConnection composition、commitText、selection。
- Clipboard、Sharesheet、Vibrator/Haptic、Intent URL。

### 10.4 Harmony

- TouchEvent、KeyEvent、IME composition 与 pointer identity。
- Pasteboard、Share、Vibrator、Want URL。

### 10.5 Desktop Native

- Mouse、Pen、Hover、RightClick、Wheel/Trackpad、Keyboard modifiers。
- 系统 Clipboard、Share fallback、Open URL、Haptic capability absent 诊断。

### 10.6 Web

- Pointer Events、Touch multi-pointer、Wheel、Keyboard、CompositionEvent。
- Clipboard API、Web Share capability、Open URL。
- 浏览器权限或激活限制返回明确 Effect 错误。

## 11. 安全、错误与生命周期

- PreviewActionPolicy 先于 CapabilityGate 检查。
- 外部 Effect 必须携带仍有效的真实用户激活凭证。
- 不支持的 Effect 只拒绝该动作；后续安全动作按 ActionList 语义继续或进入声明的错误分支。
- 文档切换、退出 Preview、App 后台/销毁时取消手势、动画、延迟动作、异步 Effect 和焦点。
- 重新进入 Preview 从文档默认状态和 entry route 开始，除非产品显式增加“保留 Preview 状态”开关；本项目不增加该开关。
- Action 错误不使 Preview 崩溃，全部进入 Debugger 和 warning surface。
- Host Adapter 不得回传未请求的数据。

## 12. Preview 进入态清理

进入 Preview 前必须统一关闭会遮挡 Preview 或继续持有输入的编辑器 surface，包括设置、导入、导出、文件菜单、Shape/Icon Picker、Asset/Prompt Center、Account/Login/Collab、字体和属性输入等。

现有工作区中 DSH 提前产生了 `close_preview_owned_overlays()` 候选补丁。该补丁不视为已采用实现；实施阶段必须逐项审查：

- 是否关闭了不该关闭的持久状态。
- Native 与 Web 的 auth/cancel side effect 是否等价。
- Phone/Tablet surface 与键盘 owner 是否同步释放。
- 是否有测试覆盖每个已关闭 surface 和重复进入 Preview。

## 13. 测试与验收

### 13.1 共享测试

- Schema round-trip 与旧文档 Golden。
- Gesture Arena 的确定性 pointer trace。
- Action parser、ActionPolicy、CapabilityGate 和 Effect result。
- Binding 的 PaintOnly/HitTest/Relayout/Navigation 分类。
- 动画关键帧、取消、反向和重复。
- 路由转场与单槽离散输入回放。

### 13.2 跨端 Trace

维护与平台无关的 JSON 输入 trace。每个 Host Adapter 必须证明：

- 同一 Tap/DoubleTap/LongPress/Pan/Swipe/Scale/Rotate 序列产生相同 SemanticEvent。
- 同一事件产生相同 state diff、route stack、animation 和 PreviewEffect。
- Hover 与 ContextMenu 按 capability 生成明确的适配结果。
- 中文 IME、硬件键盘、焦点遍历和 Back/Enter 行为符合共享契约。

### 13.3 平台测试

- iOS Swift contract tests 与 Simulator/真实设备手势、中文 IME、Share/Haptic。
- Android JUnit/Instrumentation 与真实设备 InputConnection、多指、Sharesheet。
- Harmony contract/设备验证。
- Desktop native input tests。
- Web Pointer/Composition/Clipboard/Web Share tests。

### 13.4 视觉与性能

- Hover、Pressed、Focus、弹层、导航转场和动画关键帧 visual snapshots。
- PaintOnly 动作不得触发 relayout。
- 事件与 Effect 队列有界且可取消。
- 动画只安排一个统一 redraw deadline，不为每个节点创建独立 timer。
- 在目标设备上记录 60Hz 帧预算；性能测量单独报告，不使用易抖动的 CI wall-clock 断言。

### 13.5 完成定义

一项交互只有在以下条件同时满足后，才能在支持矩阵标记完成：

1. Schema/Runtime 测试通过。
2. `op-preview-core` Trace 通过。
3. 对应 Host Adapter Trace 通过。
4. 平台 UI/Effect 验证通过。
5. Interact Tab 可创建并回读该交互。
6. AI/MCP 使用同一 schema 且校验通过。
7. 旧 `.op` Golden 无行为变化。

## 14. 实施边界与并行策略

实施计划将按以下所有权拆分，具体任务可交给 DSH：

- Jian Schema/Gesture Arena/Action Policy。
- `op-preview-core` 输入、绑定、动画、Effect 与 Debug API。
- Interact Tab 与文档命令。
- iOS Adapter。
- Android Adapter。
- Harmony Adapter。
- Desktop/Web Adapter 与跨端 Trace 汇总。

共享契约与测试 fixture 由主任务先固定；各 worker 不得在自己的平台分支引入平台私有语义。主任务负责集成、冲突处理、完整验证和分批提交。
