# 媒体输出验证记录

日期：2026-10-21。环境：Windows x64，Rust 1.96.0，FFmpeg 8.1.2 Gyan essentials。下述自动化测试输入为明确标识的合成帧与 PCM。用户随后已在真实 iPhone 上确认 AirPlay 声音、截图、录制通过；该实测反馈单独记录，不替代时长、音画偏差和异常流程专项验收。

## 已执行并通过

- `cargo test --locked --lib media:: --offline`：**13 个通过、2 个按设计忽略**。覆盖 NV12 画布偶数尺寸与不放大、固定帧率时间债务、44.1kHz 音频重采样余数、单声道映射、原始 BGRA 行步长与缓冲校验、停止时钟锁定、启动过程中停止的竞态。
- 缺少 FFmpeg 时返回带正确会话代次的 `Failed`，不读取帧、不报告保存成功。
- 保存目录指向现有普通文件时，拒绝创建输出且不改变原文件；这覆盖无效保存目录，**不等于真实 NTFS 权限拒绝或磁盘耗尽测试**。
- 非 PE 编码器文件启动失败时，保留 `.partial.mp4` 和非空诊断日志，不生成正式 `.mp4`，不发送 `Saved`。
- 正式文件已存在时拒绝覆盖，原文件内容和临时文件均保留。
- `cargo test --locked --lib media:: --offline -- --ignored --nocapture --test-threads=1`：**2 个通过**，8.30 秒。一个是正常有声/无声录制往返，一个是刻意指定不存在的 H.264 编码器触发真实 FFmpeg 进程非零退出，确认失败后临时录像及日志仍保留。
- 正常往返测试实际创建 Windows 音视频命名管道，真实 FFmpeg 试运行后选中 **h264_qsv**，完成两段约 2 秒 MP4，并用 FFmpeg 从头解码到尾；同批原始帧 PNG 写入后用 PNG 解码器读取通过。
- 对首批有声样本的 `ffprobe` 检查：H.264、320×480、30/1 FPS，视频时长 **2.033333 秒**；AAC 音频时长 **2.003 秒**；容器时长 2.033333 秒。两轨时长差约 **30ms**，不代表实机内容同步误差。

首批产物：`build/media-verification/synthetic_20261021_150454_090_0/`。
追加失败测试后的复验产物：`build/media-verification/synthetic_20261021_151537_852_0/`。

FFmpeg 可执行文件 SHA-256：

```text
1326DDE4C84FF1F96FE6B8916C5BED29E163E9B5DCCF995F6F3DB069D143EC5E
```

下载与许可证清单位于 `runtime/tools/ffmpeg/{runtime-manifest.psd1,SOURCE.txt,LICENSE.txt,README.txt}`。准备脚本使用固定上游提交内的清单验证归档及可执行文件。

## 尚未验收

- USB 真实手机截图、录制；两种来源横竖屏变化及逐文件检查不包含控件或手机外框。AirPlay 基础截图和录制已有用户实测通过反馈。
- 两种来源各自连续 30 分钟录制，内容音画偏差不超过 150ms；手机无声后恢复、本机播放静音不影响录制。
- 真实手机断线、切换来源、关闭到托盘和退出时的完整停录链路；短时编码阻塞及真实硬件驱动运行中失效。
- 文件系统权限拒绝、实际磁盘耗尽、120 秒封装超时、进程崩溃后临时录像恢复。
- 软件编码器回退的真实无硬件环境验证。当前机器的 NVENC/AMF 探测失败后 QSV 成功，未强制屏蔽 QSV 来验收 libx264 全流程。

## 统一复验入口

```powershell
# 默认只执行格式、Clippy 和普通测试；不会广播蓝牙或启动投屏。
./scripts/validate.ps1 -Offline

# 显式增加真实 FFmpeg 合成录制及失败测试。
./scripts/validate.ps1 -Offline -Media

# 显式增加真实 AirPlay 接收服务启停（不要求手机）。先关闭运行中的应用以避免端口冲突。
./scripts/validate.ps1 -Offline -Native
```

统一脚本会将原始输出保存到 `build/validation/`，并在任何检查失败时返回失败。
