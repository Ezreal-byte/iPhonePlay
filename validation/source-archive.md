# 源码归档重建验证

日期：2026-10-21。固定上游提交：`31acd8a78e7038d46c7e1a6ee987b9035cf36dde`。

## 修正

源码 ZIP 直接包含 `iPhoneMirror/` 目录，不再只包含一个需要二次解压且没有 Git 元数据的上游 ZIP。新增统一 `verify-upstream.ps1`：Git checkout 使用固定 HEAD 与已跟踪文件 clean 检查；无 Git 的源码归档使用 `iphoneplay-source-manifest.json` 校验固定提交和逐文件 SHA-256。原生构建、完整构建、FFmpeg 准备、发布打包与源码再次导出均调用同一验证入口。

## 已执行结果

- 当前原始 Git checkout：固定 HEAD 正确，已跟踪文件无修改，950 个上游文件。
- 从 Git checkout 导出完整源码目录：无 `.git`，950 个上游文件全部匹配 SHA-256。
- 从该无 Git 目录直接运行 `scripts/build-native.ps1 -Parallel 4`：**成功**，重新编译 Core、WirelessHost、DNS-SD shim、USB 配置 helper，并复制所需运行时。未复用原工作区的 CMake 缓存或原生输出。
- 源码校验故障测试：**7/7 通过**。覆盖完整归档、文件字节被改、文件缺失、错误 commit、越界相对路径、重复文件项，以及恢复后的再次验证。测试修改仅发生在独立导出目录，之后全部还原。
- 对已产生本地 `build/native` 输出的无 Git 目录再次导出：**成功**，仍只有清单记录的 950 个上游文件，不包含本地 Native build、DriverInstaller obj 或 Git 元数据。
- 再次导出后执行 ZIP 压缩／解压：**成功**，解压后的无 Git 目录仍通过全部 950 个文件的 SHA-256 校验。

开发验证目录位于 `build/source-rebuild-validation`，ZIP 往返验证目录位于 `build/source-zip-roundtrip`。它们属于测试中间产物，不进入最终便携包。最终发布会从完成后的工作区重新导出最新 Rust 源码和文档。

## 重现

```powershell
./scripts/verify-upstream.ps1
./scripts/export-source.ps1 -Destination ./build/source-check
./scripts/test-source-archive.ps1 -ArchiveRoot ./build/source-check
./build/source-check/scripts/build-native.ps1
./scripts/export-source.ps1 -ProjectRoot ./build/source-check -Destination ./build/source-repack-check
```

导出目录必须不存在或为空；脚本不会删除现有源目录。上述验证证明原生源码归档可脱离 Git 元数据构建，不表示已在干净 Windows 环境完成整个应用的安装与设备验收。
