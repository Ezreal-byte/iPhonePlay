# iPhonePlay 原生依赖基线

- iPhonePlay：0.1.1，GPL-3.0-only。
- iPhoneMirror：https://github.com/RayrenSX/iPhoneMirror
- 固定提交：`31acd8a78e7038d46c7e1a6ee987b9035cf36dde`。
- C ABI：18；无线宿主 IPC：8。Rust 不直接实现宿主 IPC。
- 原生依赖保留原导出符号、内部命名和版权信息。`iPhoneMirror/` 源码不做全局品牌替换。

## 实际使用的组件

| 组件 | 用途 | 发布位置 |
|---|---|---|
| iPhoneMirror.Core.dll | USB／AirPlay 会话、解码、D3D11 子窗口和原始帧导出 | 程序同目录 |
| iPhoneMirror.UsbConfigurationSwitch.exe | 精确设备的 USB 配置切换与恢复 | 程序同目录 |
| iPhoneMirror.WirelessHost.exe | 独立 AirPlay 接收宿主 | Wireless/ |
| dnssd.dll | 由固定提交构建的 Bonjour 兼容 shim | Wireless/ |
| AirPlayServer DLL 和配套旧版 FFmpeg DLL | 无线协议接收及解码 | Wireless/ |
| iPhoneMirror.Driver.exe | 单独发布的自包含驱动管理器 | 程序同目录 |
| FFmpeg 8.1.2 Gyan essentials | H.264／AAC MP4 录制 | tools/ffmpeg/ |

不包含原 WPF 主程序、iUsbBridge／Python USB 反控桥、DDI、UxPlay、虚拟摄像头和原版自动更新器。驱动管理器本身仍使用其上游 WPF 实现，这是首版明确保留的独立工具。

## 构建与来源记录

`scripts/build-native.ps1` 校验固定上游源码，并只构建 Core、WirelessHost、USB 配置工具及相关测试。`scripts/build.ps1` 独立调用 DriverInstaller 项目，使用 .NET SDK 10.0.301；不调用上游完整构建流水线。

USB 配置 helper 由 Core 按主进程目录查找，因此发布时将 `runtime/` 内容平铺到 `iPhonePlay.exe` 同目录。开发构建也将 helper 复制到 `target/debug` 或 `target/release`。

AirPlay 的准确二进制来源、哈希、补丁和第三方源代码记录见：

- `iPhoneMirror/third_party/airplay-server/BUILD.json`
- `iPhoneMirror/third_party/airplay-server/SOURCE.md`
- `iPhoneMirror/third_party/airplay-server/SHA256SUMS.txt`
- `iPhoneMirror/third_party/airplay-server/NOTICE-FFMPEG-BUILD.txt`

其中接收库基线提交为 `34ba6cfd49b2432cf30e89913d66decb775763e4`，选入更新提交由 BUILD.json 记录为 `85bb8dcbe82f75f40f30bb603255668f9162a8f9`。本项目不重新声称上游说明中的旧产品版本即为当前运行时版本。`dnssd.dll` 使用当前固定源码编译，故其哈希会与原捆绑文件不同。

独立录制 FFmpeg 的下载、归档 SHA-256 和各文件 SHA-256 固定在 `iPhoneMirror/scripts/ffmpeg-runtime-manifest.psd1`。`scripts/prepare-ffmpeg.ps1` 验证后复制到 runtime。FFmpeg.exe SHA-256：`1326DDE4C84FF1F96FE6B8916C5BED29E163E9B5DCCF995F6F3DB069D143EC5E`。Gyan 构建包含 GPL 编码器，相关许可证和来源随工具一起交付。

Microsoft VC++ 运行时从已安装 Visual Studio 的 redist 目录复制，使用上游 `prepare_vc_runtime.ps1` 验证 Microsoft 签名和复制后哈希。Apple 驱动不随本项目捆绑；用户通过独立管理器选择安装与修复。默认不修改系统驱动或计算机名称。

## 已知上游边界

USB B 兼容模式中的可选 SpringBoard 方向观察会尝试调用 iUsbBridge。首版排除此桥时，上游会降级为普通 B 协议选项；默认 Demo 及普通 USB 路径仍由 Core 提供。需要通过真实 iPhone 验证各兼容模式的旋转行为，不能据此宣称完整原生 USB AirPlay 模式已验收。

源代码包包含固定上游提交的完整源码目录、Rust 源码、Cargo.lock 和构建脚本，可直接解压构建，不需要 Git 元数据。`scripts/export-source.ps1` 从干净的固定 Git 提交导出上游，生成 `iPhoneMirror/iphoneplay-source-manifest.json`，记录固定提交、每个文件相对路径、字节数和 SHA-256。归档重打包只复制清单列出的上游文件，不夹带 bin、obj、work 或其他本地构建结果。

`scripts/verify-upstream.ps1` 对 Git checkout 校验 HEAD 与已跟踪文件未修改；没有 `.git` 时，严格检查归档清单的 schema、固定提交、必需文件、路径边界、重复项及每个文件的 SHA-256。所有构建与打包入口共用该校验。源码 ZIP 的整体 SHA-256 同时写入发行目录的 SHA256SUMS，便于检查下载和复制完整性。

后续若公开分发，必须同时提供与各 GPL 组件匹配的完整对应源码、依赖构建配方与补丁；本次构建不会自动上传或公开发布。
