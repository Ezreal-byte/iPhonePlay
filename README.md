# iPhonePlay

Windows 原生 iPhone 投屏应用，版本 `0.1.1`。Rust 1.96.0 / egui、eframe 0.37.0，复用固定版本 iPhoneMirror C++ Core（C ABI 18）、独立无线宿主和驱动管理器。

视觉基准为 `design/review-v2/index.html`。主程序为 `iPhonePlay.exe`，支持跟随 Windows、明亮、暗黑主题；单设备会话、AirPlay / USB 投屏、可选蓝牙键鼠控制、PNG 截图、MP4 录制。首版不包含 USB 反控、麦克风混音、直播、虚拟摄像头、自动更新。

## 使用

1. 将便携 ZIP 完整解压后运行 `iPhonePlay.exe`，或运行安装包。不要单独复制 EXE，Core 和 Wireless 依赖必须保留。
2. AirPlay：iPhone 与电脑在同一局域网，在手机的「控制中心 → 屏幕镜像」选择主界面显示的 `iPhonePlay · 电脑名`。服务就绪不等于手机已连接，收到真实视频帧才进入工作区。
3. USB：点击「使用 USB 连接」，连接数据线、解锁并信任电脑。缺驱动时由用户点击独立管理器完成安装，再重新检测；应用不静默安装驱动。
4. 蓝牙反控：投屏后点击「开启反控」，在手机上开启辅助触控，进入「辅助触控 → 设备 → 蓝牙设备」，选择引导中显示的 Windows 电脑名称。手机接入本应用的键鼠服务后会出现在确认列表中，请手动确认正在投屏的设备。Windows 的已配对记录本身不代表已接入键鼠服务。设置中的「键鼠控制」也可打开连接引导与 Windows 蓝牙设置。
5. 点击「开始控制」才会接管键鼠。**F9** 随时释放；失焦、进入设置、最小化、断线或退出也释放，恢复焦点不会自动重新接管。
6. 截图保存到系统图片目录的 `iPhonePlay` 文件夹，录像保存到系统视频目录的 `iPhonePlay` 文件夹。设置可以修改目录和录制质量。录像只包含手机声音，本地播放静音和音量不影响录像。
7. 工作区的「悬浮窗口」或「更多 → 独立悬浮窗口」将画面移入独立窗口，可拖动、缩放并保持置顶。关闭悬浮窗口会把画面收回主窗口，投屏与录制继续。设置「画面与声音」可以调整预览圆角和悬浮置顶；圆角只改变本地显示，截图与录像保留完整画面。

默认关闭主窗口会收起到托盘，投屏和录制继续，键鼠释放；托盘菜单可以重新打开或退出。退出或切换来源时，先释放输入、完成停录，再结束会话。封装失败保留 `.partial.mp4` 和编码日志，不显示成功。

配置与日志独立位于 `%LOCALAPPDATA%\iPhonePlay`。配置包含 schema 版本并原子替换；损坏或更高版本配置不会被默认配置覆盖。安装与卸载不会自动删除图片、录像或个人配置。

## 构建

目标：Windows 10 22H2 / Windows 11 x64。需要 Rust 1.96.0、MSVC C++ Build Tools 与 Windows SDK、CMake >= 3.25、PowerShell 7。驱动管理器构建需要 .NET SDK 10.0.301（源项目 `global.json`）；SDK 可以只放在 `build/tools/dotnet`。安装包使用 Inno Setup 6。

上游源码位于 `iPhoneMirror` Git 子模块，固定提交为 `31acd8a78e7038d46c7e1a6ee987b9035cf36dde`。克隆本仓库时使用 `--recurse-submodules`；已经克隆的工作区执行：

```powershell
git submodule update --init --recursive
```

手动准备上游源码时也可以使用：

```powershell
git clone https://github.com/RayrenSX/iPhoneMirror.git iPhoneMirror
git -C iPhoneMirror checkout 31acd8a78e7038d46c7e1a6ee987b9035cf36dde
```

发布脚本生成的完整源码 ZIP 解压后直接包含该版本的 `iPhoneMirror/`，不需要 Git 元数据或二次解压。GitHub 自动生成的 Source code ZIP 不包含子模块内容，需要手动准备上述固定上游源码。`scripts/verify-upstream.ps1` 对 Git checkout 校验固定 HEAD 和无已跟踪修改；对源码归档校验 `iPhoneMirror/iphoneplay-source-manifest.json` 中的固定提交及每个源码文件的 SHA-256。构建、FFmpeg 准备和再次打包均调用此校验。不要运行上游全量 `build.ps1`，它还会构建首版范围外的 WPF 主程序与 Python 桥。

```powershell
./scripts/build-native.ps1
./scripts/prepare-ffmpeg.ps1
./scripts/build.ps1
./scripts/validate.ps1 -Offline
./scripts/package.ps1
```

原生构建只生成 Core、WirelessHost、DNS-SD shim、USB 配置 helper 和相关协议测试。`build.ps1` 编译 Rust 主程序并独立发布驱动工具。依赖锁文件为 `Cargo.lock`；FFmpeg 固定为 8.1.2，下载及打包均验证 SHA-256。完整发布脚本的参数以 `Get-Help ./scripts/build.ps1 -Detailed` 和源码参数块为准。

开发运行：

```powershell
cargo build --locked
Copy-Item runtime/iPhoneMirror.UsbConfigurationSwitch.exe target/debug/
cargo run --locked --bin iPhonePlay
```

开发程序会在项目 `runtime` 目录加载原生依赖。发行目录将原生依赖平铺至 EXE 同目录，保留 `Wireless`、`tools/ffmpeg` 子目录；这满足上游 USB helper 的路径要求。可用 `IPHONEPLAY_RUNTIME` 显式指定运行时目录。

图形界面优先 wgpu；初始化失败启动独立进程尝试 glow。`--renderer=glow` 可用于显式验证回退。媒体预览始终使用原生 D3D11 子 HWND，普通显示不会逐帧上传 BGRA。

## 开发验证与演示

```powershell
./scripts/validate.ps1 -Offline -Native -Media
cargo run --bin iPhonePlay -- --demo=welcome --theme=dark
cargo run --bin iPhonePlay -- --demo=workspace --theme=light
cargo run --bin iPhonePlay -- --demo=control
cargo run --bin iPhonePlay -- --demo=settings
cargo run --bin iPhonePlay -- --demo=usb
cargo run --bin iPhonePlay -- --demo=recovery
```

演示模式显示明显标识，不创建投屏接收器，不发送手机控制或录制命令，不写个人配置。示例画面仅用于界面对照。真实运行不加载任何假连接、假视频或成功提示。

界面证据可使用 `--screenshot=<绝对 PNG 路径> --quit-after-capture`，由实际 egui 渲染器读取窗口像素；该调试功能不包含原生子 HWND 视频内容，用户截图功能另外读取 Core 原始帧。演示模式可加 `--hidden-capture` 在隐藏窗口中渲染，`--settings-tab=0..4` 选择设置分类。`--size=1280x900`、`--scale=1.25` 用于逻辑布局测试；改变应用缩放不等同于切换 Windows 显示器 DPI。

## 实施与验证状态

源码包含主流程与真实后端。自动化结果和硬件限制分别记录在 `validation/`；视觉对照记录在 `design-qa.md`。**功能实现不等于全部实机验收完成**：真实 AirPlay / USB 的 20 次启停、旋转、断线恢复与各 30 分钟录制，需要在目标 iPhone 和 Windows 硬件上完成。

2026-10-21 用户已确认真实 AirPlay 投屏、声音、截图、录制、全屏通过。开发机也已验证独立接收服务启停与真实 FFmpeg 管道输出。蓝牙启动曾因 Rust 将 Windows 的 `Aborted(3)/Success(0)` 过渡状态立即判为失败而被提前停止；修复后同机实际记录 `3/0 → 2/0`，GATT 广播启动／停止通过。广播测试本身不等于键鼠验收；用户随后在 0.1.1 中确认了实际手机连接、基本键鼠与 F9 释放，其他释放场景仍需分别验证。USB 依赖用户安装 Apple USB 支持和兼容过滤驱动，尚未完成实机验证。USB 兼容模式 B 不打包上游 Python 方向观察器，会使用 Core 自带的普通兼容路径。

0.1.1 修复未开始控制时的失焦提示覆盖配对状态，并补充手机端连接路径和设备确认入口；更新主题预览图片、Logo、字号和表单密度。用户已进一步确认手机出现在设备列表、应用内确认后键鼠正常、F9 释放正常。圆角与悬浮窗口已通过实际隐藏 HWND 生命周期验证；真实 D3D 圆角／悬浮的完整回归及其他失焦、断线释放场景仍待实机验收。

## 模块

| 路径 | 职责 |
|---|---|
| `src/app.rs` | 命令／事件整合、单会话代次、连接与退出顺序 |
| `src/ui/` | 原型布局、明暗主题、连接／工作区／控制／恢复／五类设置 |
| `src/native/` | 动态 C ABI 18、缓冲所有权、设备发现、会话和 D3D11 子窗口 |
| `src/control/` | WinRT BLE HID、绑定路由、输入队列、钩子和 F9 释放 |
| `src/media/` | NV12／PCM 到 FFmpeg、墙钟调度、封装、原帧 PNG |
| `src/settings.rs` | 版本化配置、Known Folders、校验与原子写入 |
| `src/platform.rs` | 托盘、系统窗口、目录选择和日志导出 |
| `src/floating.rs` | 独立预览窗口、DPI、置顶和安全返回主窗口 |
| `scripts/`、`installer/` | 可复现构建、验证、便携 ZIP、安装包和源码归档 |

## 开源

GPL-3.0-only，见 `LICENSE`。保留 iPhoneMirror 原作者 RayrenSX 及贡献者版权，以及全部第三方声明，见 `THIRD_PARTY_NOTICES.md`、`UPSTREAM.md` 与发行包 `licenses/`。系统字体通过 Windows 本地字体加载，不重新分发微软字体。自有引导插图、主题预览与应用图标由 ImageGen 生成；本次资产提示词见 `assets/generated-assets.json`。演示源画面来自本项目已确认的设计稿。图标库使用 Windows Segoe Fluent Icons / MDL2。

打包不包含原 WPF 主程序、USB 反控 Python 桥、DDI、自动更新器或原自动更新地址。构建与打包脚本不会自动上传或公开发布。
