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
    <a href="https://www.virustotal.com/gui/file/2d28a823564809f4d87d42c744db8282988774d8a6001e2f272c1fcafaf4396d">
      <img src="https://img.shields.io/badge/Antivirus-Scan-2EC7FF?logo=virustotal&logoColor=white" alt="VirusTotal Scan" />
    </a>
  </object>
</p>
<p align="center">
  <strong>English</strong> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.cn.md">中文</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.jp.md">日本語</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.tr.md">Türkçe</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.es.md">Español</a>
</p>
<br></br>
Aurora is a lightweight modding platform for Unreal Engine anime games allowing you to freely load Unreal Engine 5 PAK mods, Lua scripts and blueprints.
Easily start modding your favourite games with a clean desktop interface and super easy setup.
<br></br>

Easily load both `.pak` character model mods and `.asi` DLL mods into your game, with support for Lua scripts as well.

Aurora is written in [Rust](https://rust-lang.org) and rendered with [Slint](https://slint.dev).
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

<table>
<tr>
<td width="33%"><b>Easy Setup</b><br>Guided installer with automatic game path detection.</td>
<td width="33%"><b>Mod Manager</b><br>Toggle, group, filter and bulk-edit everything you've installed.</td>
<td width="33%"><b>Custom Engine</b><br>Our engine Everlight is built for stability and power while staying minimal. Also supports launch arguments.</td>
</tr>
<tr>
<td><b>Lua Scripting</b><br>Write and edit scripts in the launcher, load them in-game.</td>
<td><b>Addons Manager</b><br>Install optional QoL addons on demand, no bloat by default.</td>
<td><b>Screenshot Manager</b><br>Browse the screenshots you took in-game.</td>
</tr>
</table>

**Runs on every version** - Global, Taiwan and China, on Native (Perfect World), Steam and Epic Games. Aurora is always there.

### Inside the Mod Manager

<table>
<tr>
<td width="33%"><b>Toggling</b><br>Enable or disable with one click</td>
<td width="33%"><b>Renaming</b><br>Rename the mod on disk</td>
<td width="33%"><b>Deleting</b><br>One click, with a confirmation prompt</td>
</tr>
<tr>
<td><b>Searching</b><br>Find installed mods by name</td>
<td><b>Filtering</b><br>By enabled state, author or character</td>
<td><b>Icons</b><br>Built-in library or a custom image</td>
</tr>
<tr>
<td><b>Views</b><br>List or grid layout</td>
<td><b>Groups</b><br>Collapsible drag and drop groups</td>
<td><b>Bulk Actions</b><br>Toggle, rename or delete many at once</td>
</tr>
<tr>
<td><b>Automatic Mod Updating</b><br>Mods installed from GameBanana have version tracking</td>
<td><b>Notify on Incompatibility</b><br>Aurora notifies about incompatible mods</td>
<td><b>Restart Flags</b><br>Marks mods that only apply restarting the game</td>
</tr>
</table>
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
> In order to build the application yourself, you need the following tool: [Rust Programming Language](https://rust-lang.org)

> [!WARNING]
> View our [license](https://github.com/Daturaxoxo/Aurora/blob/main/LICENSE) before building from source!


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
