# 媒体输出模块

`Recorder` 的 `start`、`stop`、`screenshot` 均不阻塞 UI。`try_event` 返回带会话代次的结果。`Started` 包含单调时钟 `started_at`，界面用它显示录制时间；`Finalizing` 后需要等 `Saved`、`Failed` 或 `Cancelled` 再断开原生会话。`FrameSource` 已为 `NativeSession` 实现，返回的音视频内存全部由 Rust 持有。

视频与音频使用独立的 Windows 重叠 I/O 命名管道，句柄不继承，拒绝远程客户端；取消后等待 I/O 完成再销毁缓冲区。每一路最多保留一帧或 100ms PCM，FFmpeg 输入队列另有限额。固定帧率由共享 `Instant` 调度，落后时保留时间债务、重复最新帧；音频缺口补零。音频只读取 Core 原始 PCM，完全独立于本地播放音量。

录制开始前，以真实输出尺寸试运行 NVENC、AMF、QSV，最后回退 libx264。画布由首帧确定，Core 的 NV12 导出负责旋转后的等比例补边；Core 全范围 BT.709 NV12 经 FFmpeg 转成标准视频范围。输出是 H.264/AAC MP4。正常退出、文件同步之后才用不会覆盖已有文件的 `MoveFileW` 将 `.partial.mp4` 发布为 `.mp4`。结束最多等待 120 秒，失败保留临时文件与 `.partial.ffmpeg.log`。

`scripts/prepare-ffmpeg.ps1` 复用固定 iPhoneMirror 提交的下载清单及 SHA-256 校验。当前运行时：FFmpeg 8.1.2 Gyan essentials，`runtime/tools/ffmpeg/ffmpeg.exe` SHA-256 为 `1326DDE4C84FF1F96FE6B8916C5BED29E163E9B5DCCF995F6F3DB069D143EC5E`。该目录的许可证、README、SOURCE、runtime-manifest 必须一起分发。

验证命令：

```powershell
cargo test --lib media:: --offline
cargo test --lib media::integration::ffmpeg_records_video_and_phone_audio_then_decodes --offline -- --ignored --nocapture
```

2026-10-21 本机验证：合成输入通过有音频/无音频两个真实 FFmpeg 管道录制、完整解码及 PNG 解码；使用 Intel QSV 编码器。ffprobe 检查有声文件为 H.264 320×480 / 30FPS、AAC，视频 2.033333 秒、音频 2.003 秒。产物保留在 `build/media-verification/synthetic_20261021_150454_090_0/`。这不代表 AirPlay、USB、实际手机旋转、磁盘耗尽、编码器运行中失效及 30 分钟音画同步已通过实机验收。
