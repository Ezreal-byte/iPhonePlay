# 蓝牙与键鼠控制验证

验证日期：2026-10-21。当前验证版本：0.1.1。运行环境：本机 Windows x64，Rust 1.96.0，windows-rs 0.62.2。

## 自动化结果

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib control:: --offline` | 21 通过，0 失败，2 个显式环境测试默认忽略；共 23 项控制测试 |
| `cargo clippy --lib --offline -- -D warnings` | 当前控制修复后通过，无警告 |
| `cargo test --lib control::transport::tests::windows_paired_devices_readonly_diagnostic --offline -- --ignored --nocapture` | 单独执行：1 通过；只枚举系统配对记录 |
| `cargo test --lib control::transport::tests::windows_service_start_stop_smoke --offline -- --ignored --nocapture` | 当前控制修复后单独执行一次：1 通过；真实 BLE 服务创建、广播、释放完成，测试阶段用时 0.10 秒 |
| `cargo check --lib --offline --message-format short` | 此前工程集成阶段通过；本次已由测试编译和 Clippy 覆盖库编译 |
| `cargo check --bin iPhonePlay --offline --message-format short` | 此前控制集成修改后通过；本次控制模块未修改应用接口 |

已覆盖：标准 Bluetooth UUID、上游 HID report-map 版本、鼠标有符号边界和 Boot 转换、键盘修饰键释放、6 键 rollover 恢复、明确确认后才能绑定、同名设备不自动选择、投屏身份改变清除绑定、旧输入代次失效、恢复焦点不自动接管，以及 1,000 次移动合并后 2,000 个按钮边缘事件均完整保序。另验证 Paused 缺少已确认客户端时不可判定就绪，以及工作线程已经退出时能确认 shutdown 完成。

本次新增四项广播回归：启动中的 `Aborted/Success` 等待后续 `Started`；广播停止时仍有真实活跃 HID 订阅可保留连接；只有配对记录不能视为启动成功；`StartedWithoutAllAdvertisementData` 与上游一样视为广播已启动。

在上述 15 项基础上，本轮再增加 6 项回归：非 Active 状态失焦时保持原步骤及消息；Active 失焦进入 Paused 且恢复焦点不自动接管；配对时显式 Pause 保留连接指引；新订阅手机进入等待确认且不自动绑定；绑定设备断开后回到等待连接、不转控另一台手机、原设备重连仍需主动开启；动态读取按请求客户端返回已协商协议与滚轮设置，并返回最新按键报告。

## 本轮引导状态与 GATT 修复

- 修复普通失焦将 WaitingPairing / WaitingConfirmation 覆盖为“窗口失去焦点，键鼠已释放”的问题。Release / Pause 始终关闭输入闸、清理本地按键及待发移动；仅原状态为 Active 时发布 Paused。未接管输入时保留当前引导或错误状态。
- 只有尝试发送过真实输入时才发送远端零状态释放报告。配对和确认阶段不发送合成探测通知，避免打断仍在建立的 HID 订阅；实际控制后的释放保护继续保留。
- 对六个输入特征注册 `SubscribedClientsChanged`，回调仅记录内存计数，后台线程枚举订阅。日志记录每类订阅数、非 Active 会话数、有效客户端数及动态读写事件，不将系统已配对清单伪装成可控制的客户端。
- 动态 ProtocolMode、滚轮倍率和输入报告通过 `ReadRequested` 返回当前值；只有常量信息继续使用 `StaticValue`。无响应写入不调用多余的 `Respond`。微软文档说明常量值由系统应答，因此读取事件计数为 0 不能证明手机没有读取静态 ReportMap。[Windows GATT 读取说明](https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/gatt-server#read)
- 当前修改没有改变 HID UUID、Report Map、GATT 特征创建顺序，也没有自动解除或重建系统配对。

## 本机实际蓝牙能力检查

显式执行：

```powershell
cargo test --lib control::transport::tests::windows_service_start_stop_smoke --offline -- --ignored --nocapture
```

**修复后的结果：实际 BLE 服务启动/停止通过；完整手机反控尚未验收通过。**

最初的烟雾测试和用户连接尝试返回 `Aborted (3) / Success (0)`。后续逐行对照固定上游的 `BluetoothHidMouseService.cs::OnAdvertisementStatusChanged`，确认这是 **Rust 移植中的启动时序错误**：上游明确允许 MediaTek/Windows 在替换广播时先报告 `Aborted/Success`，随后报告 `Started`，并在启动的五秒等待窗口内忽略该中间状态。旧 Rust 实现第一次看到 `Aborted` 就调用 `StopAdvertising`，主动终止了本可成功的启动。因此，之前的失败不能作为本机适配器不支持 BLE HID 的证据。

当前引导状态和动态读取修复完成后，确认没有 iPhonePlay 应用运行，显式执行一次上述真实 WinRT 烟雾测试，得到：

```text
Actual BLE HID service ready; advertisement events: 3/0 → 2/0;
status: Ok(GattServiceProviderAdvertisementStatus(2)); subscribed clients: Ok([])
test control::transport::tests::windows_service_start_stop_smoke ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 49 filtered out; finished in 0.10s
```

这直接验证了 `Aborted/Success → Started/Success` 的实际状态转换，最终状态为真实 `Started (2)`，没有使用模拟数据或能力标志代替广播结果。`IsConnectable`、`IsDiscoverable` 都为 `true`，`ServiceData` 保持未设置，与上游一致；本次没有修改这些参数、HID UUID 或 Report Map。

烟雾测试的活跃客户端列表为空。本次验证没有接管或确认任何 iPhone；用户已有 Windows 配对记录不能替代本服务的活跃 GATT 订阅。因此以下仍需用户在修复版本中复测：真实手机订阅与应用内确认、鼠标/滚轮/键盘通知、F9 的真实远端释放、Alt-Tab/设置/托盘/蓝牙断线后的远端按键状态，以及不同适配器兼容性。

另通过只读 WinRT 枚举得到 Classic 已配对记录 3 项、BLE 已配对记录 4 项，其中用户的 iPhone 同时存在于两类清单。因此不能将当前问题归因为“只完成了经典蓝牙配对”。这些记录仍不能证明手机订阅了当前 iPhonePlay HID 服务。用户反馈时的旧日志仅包含广播 `3/0 → 2/0` 成功，缺少订阅细节；新版增加的日志需要结合用户手机端重新连接结果继续判断，尚未确认空客户端列表的最终原因。

本次仅通过程序调用自身 GATT 服务完成一次启动/停止验证；测试结束前销毁 Transport 并解除事件注册、停止广播，测试进程已正常退出。未启动可见窗口，未使用桌面自动化，未修改 Windows 蓝牙开关、配对记录、电脑名称或驱动。

## 实现边界与集成要求

- UI 发送命令不执行 WinRT 调用；`release` 同步关闭原子输入闸，实际零状态报告由后台线程发送。输入钩子线程在 4ms 轮询中恢复本机光标，无需等待蓝牙超时。
- 激活必须同时具有已确认的当前客户端、有效投屏会话、投屏区域焦点、有效物理屏幕预览矩形，以及前台根 HWND。设置页、最小化、失焦、断线及退出应先调用 `release`。
- `SetWindow` 使用根 HWND，`SetVideoRect` 使用物理屏幕坐标。预览子 HWND 不改变顶层前台窗口。进入控制时本机指针锚定预览中心，F9 退出。
- `ControlSnapshot.session_id` 是投屏会话代次；UI 应过滤过期快照，停止后的 `Off` 快照代次为 0。
- `Stop` 是可复用的后台停止命令。应用退出使用 `request_shutdown()` 并逐帧查询 `shutdown_complete()`，后者以工作线程实际结束为依据，初始化失败或已经退出的线程也能确认。UI 同时等待原生后端退出确认；原生后端超时作为失败记录，不计为正常完成。`Drop` 具有兜底释放和线程回收；若正在执行 WinRT 创建服务，可能需等待该操作的有限超时。创建每项 GATT 特征及广播等待中都会检查退出/会话取消。
- 应用集成会调用 Windows `SetFocus`，再以 `GetFocus` 验证原生预览 HWND 已取得焦点，之后才提交显式 Activate；无法取得焦点时保持本机输入。
- 单条普通通知最多等待 300ms；释放报告每条最多 120ms。通知超时或释放失败会停止广播并关闭实际 GATT 会话，避免旧通道重放和残留键状态。
- 首次绑定与重新绑定必须显式调用 `ConfirmClient`。设备消失时保留其身份，绝不会切换到其他已订阅设备；应用内解除绑定不删除 Windows 配对记录。
- 广播状态与现有 GATT 连接分别检查。只有本服务的 `GattSubscribedClient` 且其 `GattSessionStatus` 为 `Active` 才算活跃订阅；广播停止但仍有这样的客户端时保留连接，不能单凭 `Aborted/Success` 误判断线。无广播、无活跃订阅持续五秒才报失败，记录状态事件序列，并退役失效服务以便再次启动。

## 上游兼容性

HID report-map 与报告定义来自固定基线 `31acd8a78e7038d46c7e1a6ee987b9035cf36dde` 的 `BluetoothHidMouseService.cs`，保留 report-map 版本 3。包括键盘、鼠标、滚轮分辨率 feature、Consumer 和 Navigation 报告，以及 Boot Keyboard/Mouse 特征。配对服务使用加密读取/写入，通知始终指定当前确认客户端。

## 2026-10-21 用户实机复测

用户在 0.1.1 中按手机端连接步骤操作后，明确反馈「已出现，可以确认」；随后确认「键鼠正常，F9 释放正常」。因此实际手机发现、应用内确认、基本键鼠控制与 F9 释放已获用户实机验收。上文空订阅描述保留为较早烟雾测试和排障时的历史证据，不再代表最终使用结果；其他失焦／托盘／断线释放和跨适配器测试仍待单独验收。
