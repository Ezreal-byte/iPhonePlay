# iPhonePlay 第三方声明

iPhonePlay 0.1.1 使用 GPL-3.0-only。原 iPhoneMirror 代码版权属于 RayrenSX 及其他贡献者，依赖固定于提交 `31acd8a78e7038d46c7e1a6ee987b9035cf36dde`。本项目保留原生 DLL、无线宿主和独立驱动管理器的版权与许可，不以新品牌替换其作者归属。

每个第三方组件继续遵循其各自许可。根目录 LICENSE 不替换或缩减以下许可。发布文件中的 `licenses/` 目录、FFmpeg 工具目录和源代码记录构成本声明的一部分。

| 组件 | 用途 | 许可与来源 |
|---|---|---|
| iPhoneMirror Core、WirelessHost、DnsSdShim、USB helper、DriverInstaller | 原生捕获、无线宿主、驱动管理 | GPL-3.0-only；https://github.com/RayrenSX/iPhoneMirror |
| libusb 1.0.29 | USB 传输 | LGPL-2.1-or-later；https://github.com/libusb/libusb |
| libusb-win32 1.2.6.0 | USB 用户态及独立驱动工具负载 | 上游 LGPL/GPL 文本及 AUTHORS 随包保留；https://github.com/mcuee/libusb-win32 |
| AirPlayServer 及 PlayFair | AirPlay 接收 | MIT / GPL-3.0，准确组合、补丁和来源见 upstream/AirPlayServer-SOURCE.md |
| 无线接收用 FFmpeg DLL | H.264／ALAC 解码 | LGPL-2.1-or-later，完整构建与运行时许可见 upstream/AirPlayServer-NOTICE-FFMPEG-BUILD.txt |
| Fraunhofer FDK AAC | 无线 AAC 解码 | Fraunhofer FDK AAC license，保留 upstream/AirPlayServer-NOTICE-FDK-AAC.txt |
| FFmpeg 8.1.2 Gyan essentials | 独立视频录制编码器 | GPLv3 构建；tools/ffmpeg/LICENSE.txt、README.txt、SOURCE.txt |
| WPF-UI 4.3.0 和 Fluent System Icons | 独立驱动管理器界面 | MIT；licenses/driver/ 下保留原许可与第三方声明 |
| Microsoft .NET Windows Desktop runtime | 自包含驱动管理器 | MIT 及其第三方组件许可；https://github.com/dotnet/windowsdesktop |
| Microsoft Visual C++ Redistributable | 原生运行时依赖 | Microsoft 软件许可；仅复制已验证 Microsoft 签名的 app-local redistributable 文件 |
| egui／eframe 0.37.0、wgpu、winit、windows-rs 等 Rust crates | 原生 Rust 界面与平台功能 | 具体版本和许可见 Cargo.lock 与 licenses/RUST-DEPENDENCIES.md；各 crate 的原始许可文本保留在 licenses/rust/ |
| Inno Setup | 安装包引擎 | Inno Setup License；安装包中保留 licenses/Inno-Setup-LICENSE.txt |

Rust 依赖许可索引由打包脚本从锁定依赖图生成，包含传递依赖。系统中文字体在运行时从 Windows 字体目录加载，本项目不重新分发 Windows 字体。原型参考图不是运行时软件画面。

独立录制工具使用 Gyan 的固定 FFmpeg 8.1.2 essentials 包，和上游默认的自编译 compact profile 不同。下载地址、归档与可执行文件哈希均固定并在打包前再次验证。FFmpeg 以独立进程运行；H.264 的 libx264 等 GPL 组件及依赖维持各自许可。对应来源与构建信息应随任何后续公开二进制分发一起提供。

`licenses/upstream/iPhoneMirror-THIRD_PARTY_NOTICES.md` 为完整保留的上游历史声明，其中某些功能（Python USB 反控桥、UxPlay、虚拟摄像头、Markdig 更新页面）未被打包到本产品。该历史声明不表示 iPhonePlay 包含这些功能。

Apple 软件不在本项目中重新分发。驱动管理器中的 Apple 支持安装流程使用上游提供的官方来源与签名检查。iPhone、AirPlay 及相关名称属于其各自权利人；iPhonePlay 是独立项目。

所有第三方组件按其许可证提供，不附加新的担保。
