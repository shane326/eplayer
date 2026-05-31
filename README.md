# ePlayer

ePlayer 是从现有 MoonTV 桌面端拆出的新应用，播放器层改为 WebView + hls.js，不再依赖 mpv/libmpv。

当前范围：

- MacCMS 点播源导入、片库、搜索、详情、选集
- 直播源导入、M3U/TXT 解析、频道分组和播放
- WebView 内置 hls.js 播放 HLS
- 本地 m3u8 代理、分片重写和广告片段过滤
- 播放历史、历史续播
- 跳过片头片尾、倍速、进度拖动、空格暂停、左右快退快进、播放器全屏

用户数据目录：

- Windows: `%APPDATA%\ePlayer`
- macOS: `~/Library/Application Support/ePlayer`
- Linux: `$XDG_CONFIG_HOME/ePlayer` 或 `~/.config/ePlayer`

运行：

```powershell
cargo run
```

构建：

```powershell
cargo build --release
```

Windows 需要 WebView2 Runtime。Windows 11 和大多数新版 Windows 10 通常已内置；如果打不开窗口，需要安装 Microsoft Edge WebView2 Runtime。
