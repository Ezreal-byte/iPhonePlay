# 原生集成测试记录

日期：2026-10-21。环境：Windows x64，MSVC 19.51.36247.0，Windows SDK 10.0.26100.0，Rust 1.96.0。固定原生提交：`31acd8a78e7038d46c7e1a6ee987b9035cf36dde`。

## 已执行

1. 独立 CMake 构建 Core、WirelessHost、DnsSdShim、USB 配置 helper 成功。未构建或依赖原 WPF 主程序。
2. 相关原生 CTest **12/12 通过**，11.22 秒：USB 设备重连、Core 协议、QuickTime USB 传输、libusb0 激活策略、输出模式、USB 配置恢复策略、USB 配置 helper 设备身份、IPC writer、DNS-SD 注册策略、无线宿主 smoke、运行时预检、无效 DLL 预检。原始日志：`build/native/Testing/Temporary/LastTest.log`。
3. Rust 原生测试 **5/5 通过**：ABI 18 的 7 个结构体大小及关键字段偏移，UTF-16／内嵌 NUL 检查，缺失依赖的精确路径，缺少 DLL 后可再次初始化而不残留占用，以及有真实视频帧才认定已投屏。
4. 显式运行默认忽略的 `native::tests::native_receiver_start_stop_smoke`：**通过，最终复测 1.18 秒**。加载真实 Core DLL、检查 ABI 18 和 EnvironmentInfo 结构、启动独立真实 AirPlay 宿主、等候 `running=true && ready=true`，最后停止宿主和后端。此测试没有模拟 receiver ready。
5. 微软 VC++ 运行时从 Visual Studio redist 目录复制，Microsoft Authenticode 签名和复制后 SHA-256 检查通过。版本 14.51.36247.0。
6. 独立 DriverInstaller 使用本地 .NET SDK 10.0.301 发布成功，x64、自包含、单文件，`iPhoneMirror.Driver.exe` 为 65,537,635 字节。此项仅验证发布成功，未运行驱动安装或修改系统驱动。
7. 无 Git 元数据的源码归档重新构建 Core、WirelessHost 和 USB helper 成功，950 个固定上游文件均通过 SHA-256；完整故障和 ZIP 往返验证见 `source-archive.md`。

用户随后实测反馈 AirPlay 投屏、手机声音、截图、录制和全屏可用。该反馈记录为对应基础功能的实机通过；未记录连续启停次数、录制时长或音画偏差，因此以下专项验收仍保留待测。

## 0.1.1 圆角与独立悬浮窗口

本次新增接口和行为：

- `PreviewWindow::reparent(parent)` 将同一个预览子 HWND 转移到另一宿主，保持原会话句柄。转移时隐藏预览并清除旧布局缓存，调用方重新定位后显示；同时处理 Win32 `SetParent` 返回 NULL 但 `LastError=0` 的成功情况。
- `PreviewWindow::set_corner_radius(radius_px)` 接收物理像素，使用 `SetWindowRgn` 做显示裁切；`0` 清除圆角，改变预览尺寸时重新生成区域。已安装区域由 Windows 接管，安装失败则自行释放 GDI 对象。此接口不修改 BGRA／NV12 原始帧，也不改变截图和录制画布。
- `FloatingWindow::new(parent, title, source)` 创建独立顶层视频宿主，提供 `hwnd`、`client_size`、`dpi`、`is_minimized`、`set_topmost`、`set_title` 及关闭／点击／布局状态读取接口。窗口无 owner，主窗口最小化不会通过 Win32 所属关系连带隐藏它；默认置顶，以 `SW_SHOWNOACTIVATE` 显示，重复设置相同置顶状态不会重复调整窗口。
- `fit_video_rect(client, source)` 在宿主客户区内保持源宽高比并居中。窗口消息只更新状态或窗口布局，不调用投屏、控制或录制后端。`WM_CLOSE` 只请求返回主窗口，由主状态层先重新挂接预览，再销毁空宿主；销毁保护也会尝试转回仍留在宿主中的子窗口。

已执行的新增验证：

1. 预览圆角边界纯几何测试 **1/1 通过**，覆盖关闭圆角、小尺寸、大半径和数值边界。
2. 悬浮窗口测试 **3/3 通过**，包含两个几何／DPI／UTF-16 测试，以及显式运行、默认忽略的 `floating::tests::hidden_preview_reparent_rounding_lifecycle_smoke`。最终整组运行耗时 0.04 秒。
3. 隐藏 HWND 烟测实际创建隐藏宿主和预览子窗口，验证圆角区域添加、缩放后区域边界更新、清除圆角；覆盖关闭请求不销毁宿主或预览、预览接回后销毁宿主、遗漏显式接回时的 Drop 保护，以及旧 parent 为 NULL 时的重新挂接。测试发现并修复 GDI 圆角区域尾部少一行／列像素的问题。
4. 同一隐藏烟测确认宿主无 owner，连续切换或重复设置置顶状态始终保持隐藏，前台窗口句柄不变。测试完成后测试窗口均已销毁，未显示可见窗口、未启动投屏会话、未改变用户运行中的应用。
5. 修改后 `cargo check --locked --all-targets --offline` 和 `cargo clippy --locked --all-targets --offline -- -D warnings` **通过**。

以上验证覆盖真实 Win32 窗口及 GDI 区域生命周期，**未加载真实 D3D11 投屏画面到悬浮窗口**。悬浮模式下的 AirPlay／USB 画面、圆角实际呈现、主窗口最小化后的持续显示、跨显示器 DPI、横竖屏变化，以及录制进行中反复悬浮／接回后的画面与声音连续性仍需实机验证，未标记为验收通过。

## 尚未完成的实机验证

- 真实 USB 首帧与 D3D11 画面；AirPlay／USB 的焦点和跨 DPI 变化。
- 每种来源连续 20 次连接／断开、横竖屏切换、断线恢复。
- USB 驱动缺失／修复后的实际设备检测、Demo/B/C 兼容模式。
- 蓝牙真实 iPhone 配对、鼠标键盘报告、F9／失焦／断线释放。
- 每种投屏来源 30 分钟录制、音画偏差不超过 150 ms。
- 干净 Windows 10 22H2／Windows 11 环境的安装、运行和卸载。

宿主运行不等于手机连接。本次无手机烟雾测试只证明独立原生运行时可以启动、报告服务就绪并关闭，不将 P0 的真实投屏完成条件或整体验收标记为通过。

## 重现命令

```powershell
./scripts/build-native.ps1 -RunTests
cargo test --locked --lib native::
$env:IPHONE_MIRROR_LOG_FILE = Join-Path $env:LOCALAPPDATA 'iPhonePlay/logs/native-smoke.log'
cargo test --locked --lib native::tests::native_receiver_start_stop_smoke -- --ignored --nocapture --test-threads=1
cargo test --locked --offline --lib native::preview::tests
cargo test --locked --offline --bin iPhonePlay floating::tests -- --include-ignored --test-threads=1 --nocapture
```
