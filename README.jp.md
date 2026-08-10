#### The translation for this page isn't created yet! Be the first one to translate it and open a pull request!

#### このページの翻訳はまだ作成されていません！ ぜひ最初に翻訳して、プルリクエストを送ってください！

<p align="center">
  <img src="https://host.getaurora.moe/assets/web/embed.png" alt="ᅠ" />
</p>
<div align="center">
  
# Aurora Launcher
Lightweight modding platform for anime games.
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
  <a href="https://github.com/Daturaxoxo/Aurora/blob/rewrite/README.md">English</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/rewrite/README.cn.md">中文</a> | <strong>日本語</strong> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.tr.md">Türkçe</a> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.es.md">Español</a>
</p>
<br></br>
Aurora is a lightweight modding platform for Unreal Engine anime games allowing you to freely load Unreal Engine 5 PAK mods, Lua scripts and blueprints.
Easily start modding your favourite games with a clean desktop interface and super easy setup.
<br></br>

Easily load both `.pak` character model mods and `.asi` DLL mods into your game, with support for Lua scripts as well.

Aurora is written in [Rust](https://rust-lang.org) and rendered with [Slint](https://slint.dev), both being extremely lightweight languages.
<br></br>

> [!NOTE]
> Because our application is not signed, Windows Defender might false flag Aurora on first launch. We do not support Microsoft's [Smart App Control](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview) system. You are most likely to get blocked by Smart App Control while running Aurora for the first time.
>
> Find out how to disable Smart App Control: [Click Me!](https://docs.getaurora.moe/hidden/guides/smart-app-control)
<br>
</br>

> [!IMPORTANT]
> Aurora does not and will never support `.ini` mods, those mods are loaded in by D3D11 hooking projects like 3DMigoto or XXMI. It's close near impossible creating support for them, the architecture between Aurora and 3DMigoto is vastly different. We are an Unreal Engine PAK loader, not a DirectX hook.
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=xfE5l4OXJWrc&format=png&color=000000" height="24" alt=""> Features
</h2>

- **Easy Setup** — Aurora is designed to be friendly towards beginners, we include an installer to setup everything and our pathfinding module finds your game installation automatically.
- **Support for all versions** — We support all versions and providers:
- - **Versions** — Global, Taiwan, China
- - **Providers** — Native (Perfect World Client), Steam, Epic Games
- **Builtin Mod Manager** — Simple enough for beginners, advanced enough for people who know what they're doing:
- - **Toggling** — Disable & Enable your mods with one-click
- - **Renaming** — Rename the mod's name on disk inside the mod manager
- - **Deleting** — Delete the mod with one-click, show a popup to make sure no accidental deletions happen.
- - **Searching** — Search mods installed by name
- - **Filtering** — Filter mods by enabled/disabled status, author or character.
- - **Icons** — Select a character's icon from our built-in library, or use a custom image.
- - **Viewing Styles** — Choose between a list view or a grid view of how mods are displayed depending on your style
- - **Mod Groups** — Create groups and drag & drop mods into them, collapsable by default.
- - **Bulk Selection** — Do actions like renaming, deleting or toggling on multiple mods at once.
- **Screenshot Manager** — View the screenshots you took in-game
- **Lua Scripting** — Create and edit lua scripts from the launcher and load them in-game.
- **Addons Manager** — Install useful Quality of Life addons directly from the launcher, not installed by default to reduce bloat.
- **Custom Engine** — Powered by our custom engine **Everlight**, get total control over your game. Has tons of security checks builtin to prevent any crashes.
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=gXoJoyTtYXFg&format=png&color=ffffff" height="24" alt=""> Windows Installation
</h2>

### Installer
Easiest and the recommended way of installing Aurora.
1. Download the Windows installer from the [latest release](https://github.com/Daturaxoxo/Aurora/releases/latest)
2. Run it and configure your setup.
3. Aurora will launch automatically after installation finishes.

### Portable
1. Download the Portable Windows version from the [latest release](https://github.com/Daturaxoxo/Aurora/releases/latest)
2. Extract the ZIP file into wherever you want to install Aurora.
3. Run Aurora.exe
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=100&id=17842&format=png&color=000000" height="24" alt=""> Linux Installation
</h2>

> [!NOTE]
> Although Aurora has a native Linux build, you must run the game using DW-Proton to make sure the game actually opens. We recommend using DW-Proton-10.0-26 instead of the latest versions of DW-Proton.

> [!IMPORTANT]
> Aurora will not work correctly under root and/or sudo permissions. If your game installation is in a root-access folder, we recommend moving it somewhere else.

1. Download the Linux Portable Version from the [latest release](https://github.com/Daturaxoxo/Aurora/releases/latest).
2. Extract the ZIP file into wherever you want to install Aurora.
3. Launch Aurora.AppImage
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=keI1M862UTP2&format=png&color=000000" height="24" alt=""> Building from Source
</h2>

> [!IMPORTANT]
> In order to build the application yourself, you need the following tools: [Rust Programming Language](https://rustlang.org), [LLVM Compiler](https://www.llvm.org)
1. Download the ZIP source code of this project. (**Code Button > "Download ZIP"**)
2. Extract source code to your desired path.
3. Open an administrator command prompt in the root folder of the project.
4. Run `cargo run`
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=114474&format=png&color=000000" height="24" alt=""> Tools for Mod Creators
</h2>

Aurora has a variety of tools that mod creators can use from debug windows to dump keybinds. It also supports metadata files (`mod.json`) that you can put 
inside your mod so our mod manager can display more information about your mod. A few example include mod version, icon, (select from included icons or set your
own custom icon using an external URL), author and support link.

Learn how to [create a Display file](https://docs.getaurora.moe/mod-authors/displaying-your-mod)
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=31016&format=png&color=000000" height="24" alt=""> Translating Aurora
</h2>

We appreciate all help we can get about translations. If you'd like to contribute to the translations of our app, website or even this README file: Feel free to
check out our translators docs:

Learn how to [contribute to Aurora with translations](https://docs.getaurora.moe/translations/translation-status)
