<p align="center">
  <img src="https://host.getaurora.moe/assets/web/embed.png" alt="ᅠ" />
</p>
<div align="center">
  
# Aurora 启动器
面向二次元游戏的轻量级模组平台。
</div>
<p align="center">
  <img src="https://img.shields.io/github/v/release/Daturaxoxo/Aurora?include_prereleases&color=007ec6&v=1" alt="Release" />
  <img src="https://img.shields.io/github/downloads/Daturaxoxo/Aurora/total?color=f1ff2e&v=2" alt="GitHub Downloads" />
  <img src="https://img.shields.io/github/contributors/Daturaxoxo/Aurora?color=ff2ef1&v=3" alt="Contributors" />
  <object data="https://getaurora.moe" type="text/html">
    <a href="https://getaurora.moe">
      <img src="https://img.shields.io/badge/Official-Website-blue&color=FFFFFF?logo=cloudnativebuild&v=0" alt="Website Link" />
    </a>
  </object>
  <object data="https://virustotal.com" type="text/html">
    <a href="https://www.virustotal.com/">
      <img src="https://img.shields.io/badge/Antivirus-Scan-2EC7FF?logo=virustotal&logoColor=white" alt="VirusTotal Scan" />
    </a>
  </object>
</p>
<p align="center">
  <a href="https://github.com/Daturaxoxo/Aurora/blob/rewrite/README.md">English</a> | <strong>中文</strong> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.jp.md">日本語</a> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.tr.md">Türkçe</a> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.es.md">Español</a>
</p>
<br></br>

Aurora 是一款面向虚幻引擎二次元游戏的轻量级模组平台，让你自由加载虚幻引擎 5 的 PAK 模组、Lua 脚本与蓝图。
简洁的桌面界面搭配极其简单的配置流程，让你轻松开始为喜爱的游戏制作与安装模组。
<br></br>

轻松将 `.pak` 角色模型模组与 `.asi` DLL 模组加载进游戏，同时也支持 Lua 脚本。

Aurora 使用 [Rust](https://rust-lang.org) 编写，并通过 [Slint](https://slint.dev) 渲染界面，两者都极为轻量。
<br></br>

> [!NOTE]
> 由于我们的程序未经签名，Windows Defender 可能会在首次启动时对 Aurora 产生误报。我们不支持微软的[智能应用控制](https://learn.microsoft.com/zh-cn/windows/apps/develop/smart-app-control/overview)（Smart App Control）机制，首次运行 Aurora 时很可能会被它拦截。
>
> 了解如何关闭智能应用控制：[点我！](https://docs.getaurora.moe/hidden/guides/smart-app-control)
<br>
</br>

> [!IMPORTANT]
> Aurora 不支持、也永远不会支持 `.ini` 模组。这类模组是由 3DMigoto、XXMI 这类 D3D11 钩子（hook）项目加载的。为它们提供支持几乎是不可能的——Aurora 与 3DMigoto 的架构差异极大。我们是虚幻引擎 PAK 加载器，而不是 DirectX 钩子。
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=xfE5l4OXJWrc&format=png&color=000000" height="24" alt=""> 功能特性
</h2>

- **简易配置** — Aurora 对新手非常友好，我们提供安装程序帮你完成全部设置，路径查找模块还会自动定位你的游戏安装目录。
- **全版本支持** — 我们支持所有版本与平台：
- - **版本** — 国际服、台服、国服
- - **平台** — 原生客户端（完美世界客户端）、Steam、Epic Games
- **内置模组管理器** — 对新手足够简单，对老手足够强大：
- - **启用/禁用** — 一键开关模组
- - **重命名** — 在模组管理器中直接重命名磁盘上的模组
- - **删除** — 一键删除模组，并弹窗二次确认，避免误删
- - **搜索** — 按名称搜索已安装的模组
- - **筛选** — 按启用/禁用状态、作者或角色筛选模组
- - **图标** — 从内置图标库中选择角色图标，或使用自定义图片
- - **显示方式** — 可在列表视图与网格视图之间切换，按你的喜好显示模组
- - **模组分组** — 创建分组并拖放模组，默认可折叠
- - **批量选择** — 一次性对多个模组执行重命名、删除或启用/禁用等操作
- **截图管理器** — 查看你在游戏内拍摄的截图
- **Lua 脚本** — 在启动器中创建和编辑 Lua 脚本，并在游戏中加载
- **附加组件管理器** — 直接从启动器安装实用的体验优化组件；默认不安装，以减少臃肿
- **自研引擎** — 由我们自研的 **Everlight** 引擎驱动，让你完全掌控游戏。内置大量安全检查以防止崩溃。
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=gXoJoyTtYXFg&format=png&color=ffffff" height="24" alt=""> Windows 安装
</h2>

### 安装程序
最简单、也是推荐的 Aurora 安装方式。
1. 从[最新发行版](https://github.com/Daturaxoxo/Aurora/releases/latest)下载 Windows 安装程序
2. 运行安装程序并完成配置。
3. 安装完成后 Aurora 会自动启动。

### 便携版
1. 从[最新发行版](https://github.com/Daturaxoxo/Aurora/releases/latest)下载 Windows 便携版
2. 将 ZIP 文件解压到你想安装 Aurora 的位置。
3. 运行 Aurora.exe
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=100&id=17842&format=png&color=000000" height="24" alt=""> Linux 安装
</h2>

> [!NOTE]
> 虽然 Aurora 提供原生 Linux 构建，但你必须使用 DW-Proton 运行游戏，否则游戏可能无法正常启动。我们推荐使用 DW-Proton-10.0-26，而不是最新版本的 DW-Proton。

> [!IMPORTANT]
> Aurora 在 root 和/或 sudo 权限下无法正常工作。如果你的游戏安装在需要 root 权限访问的目录中，建议将其移动到其他位置。

1. 从[最新发行版](https://github.com/Daturaxoxo/Aurora/releases/latest)下载 Linux 便携版。
2. 将 ZIP 文件解压到你想安装 Aurora 的位置。
3. 启动 Aurora.AppImage
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=keI1M862UTP2&format=png&color=000000" height="24" alt=""> 从源码构建
</h2>

> [!IMPORTANT]
> 若要自行构建本程序，你需要以下工具：[Rust 编程语言](https://rust-lang.org)、[LLVM 编译器](https://www.llvm.org)
1. 下载本项目的 ZIP 源码。（**Code 按钮 > "Download ZIP"**）
2. 将源码解压到你想要的路径。
3. 在项目根目录中打开管理员命令提示符。
4. 运行 `cargo run`
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=114474&format=png&color=000000" height="24" alt=""> 面向模组作者的工具
</h2>

Aurora 提供了多种供模组作者使用的工具，从调试窗口到转储（dump）快捷键一应俱全。它还支持元数据文件（`mod.json`）——你可以把它放进自己的模组中，这样我们的模组管理器就能显示更多关于该模组的信息。例如模组版本、图标（可从内置图标中选择，也可通过外部 URL 设置自定义图标）、作者以及支持链接。

了解如何[创建展示文件](https://docs.getaurora.moe/mod-authors/displaying-your-mod)
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=31016&format=png&color=000000" height="24" alt=""> 翻译 Aurora
</h2>

我们非常欢迎任何关于翻译的帮助。如果你想为我们的程序、网站，甚至这份 README 文件的翻译做出贡献，欢迎查看我们的译者文档：

了解如何[通过翻译为 Aurora 做贡献](https://docs.getaurora.moe/translations/translation-status)
