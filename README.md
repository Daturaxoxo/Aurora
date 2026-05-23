<p align="center">
  <img src="https://i.ibb.co/yBSQVcy2/github-banner.png" alt="ᅠ" />
</p>
<h1 align="center">Aurora Launcher</h1>
<p align="center">
  <img src="https://img.shields.io/github/v/release/Daturaxoxo/Aurora?include_prereleases&color=007ec6&v=1" alt="Release" />
  <img src="https://img.shields.io/github/downloads/Daturaxoxo/Aurora/total?color=2ea44f&v=1" alt="Downloads" />
  <img src="https://img.shields.io/github/contributors/Daturaxoxo/Aurora?color=dfb317&v=1" alt="Contributors" />
</p>

> [!NOTE]
> Due to our application not having a signed certificate, Windows Defender (or other Antivirus software) may false-flag Aurora, we do not support Microsoft's [Smart App Control](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview) system. You are most likely to get blocked by Smart App Control while running Aurora for the first time.

Aurora is a light-weight high performance mod launcher built for [Neverness to Everness](https://nte.perfectworld.com/) that allows you to freely edit models inside the game using UE5 (Unreal Engine 5) PAKs.

Aurora is an open-source community project, any help with the source code is greatly appreciated. We are open to anyone who wants to help develop this project further, whether it be translations or contribution to the source-code.
### ᅠ
## Features
- **Easy Installation** — Download a release and open the executable file, that's it.
- **Plug & Play** — Aurora only requires your Neverness To Everness download location, afterwards its as easy as clicking a button!
- **Mod Management** — Mods can be disabled or enabled in the UI, allowing for easy configuration.
- **Live Monitoring** — Monitors Neverness To Everness to alert you if a change requires a game restart.
- **Overlays** — Aurora supports overlaying UI elements on top of Neverness To Everness, which can be used to show your in-game FPS, session information, etc (Still being implemented!)

> [!NOTE]
> We are open for your suggestions (as long as they make sense!) to create more features and make the experience of Aurora better and more smooth for everyone!
### ᅠ
## Installation
> [!WARNING]
> ## Notice for Linux Users
> Unfortunately, Aurora is built to only be ran on Windows Operating Systems. Although trying to run Aurora in Linux with Wine/Proton can be possible; due to the reliance on native Windows Directory Junctions and Win32 process injection frameworks Aurora will most likely not perform correctly when running under Wine, Proton or SteamOS.
> 
> Because of this, if you encounter issues while trying to run Aurora on Linux; we can't help you at the moment until a proper Linux build is created.

> [!TIP]
> It is recommened to download the following Prerequisite software to minimise errors.
> [Microsoft Visual C++ Redistributable](https://aka.ms/vs/17/release/vc_redist.x64.exe).

### ᅠ
### Portable Installation (Windows and possibly Linux via WINE or Proton)
1. Download the [latest release](https://github.com/Daturaxoxo/Aurora/releases) of **Aurora_vX.X.X.zip**.
2. Extract the portable zip file into a dedicated folder or trusted place.
3. Run `Aurora.exe` and wait for the User Interface to load.
> [!NOTE]
> Aurora will automatically attempt to find Neverness To Everness. If it fails you can try to use the button next to **"Launch"** to search your drives for Neverness To Everness.
> 
> If that doesn't find the game either, head over to the settings icon top left and under the **"Launcher"** tab, manually select your Neverness To Everness download location.
### ᅠ
### Building the Application from Source
> [!NOTE]
> For the simple average user, this is not recommended. Only build from source if you don't trust the Portable installation or plan on modifying the source code of the program.

> [!IMPORTANT]
> In order to build the project, you must have [Python 3.14.5](https://www.python.org/downloads/release/python-3145/) and [VCRedist](https://aka.ms/vs/17/release/vc_redist.x64.exe) installed on your computer.
1. Download the ZIP source code of this project. **(Code Button > "Download ZIP")**
2. Extract source code to your desired path.
3. Open a command prompt in the root folder of the project **(the one that houses build.py and main.py)**
4. Run `pip install -r dev/requirements.txt`
5. Run `python build.py` or `py build.py`
### ᅠ
## Mod Implementation to Aurora
> [!NOTE]
> Aurora is not a tool used to create mods, it is a tool to use mods. We are not experienced with modding, please ask experts in modding and not us if you are stuck.
Aurora's Mod Manager supports a `mod.json` file inside your mod folder to display custom assets in the mod manager. If you are a modder and would like to include it on your project, here is how to do it.
1. Create a new file named `mod.json`, in the same location as your `.pak`, `.utoc` and `.ucas` files.
2. Replace the contents with:
```json
{
    "Name": "Example Mod Name",
    "Version": "1.0.0",
    "Author": "Example Author Name",
    "Optionals": {
        "Support Link": "https://patreon.com/example"
    }
}
```
3. Edit the contents with whatever you want, Aurora handles the rest.
- You can replace your support link with whatever link you want, it doesn't have to be patreon.
### ᅠ
## Translation Status
| Language | Status | File | Status |
| :--- | :--- | :---: | :---: |
| <img src="https://flagcdn.com/w20/gb.png" width="20"> English | ![100%](https://geps.dev/progress/100?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `en.json` |
| <img src="https://flagcdn.com/w20/ru.png" width="20"> Russian | ![0%](https://geps.dev/progress/0?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `ru.json` | No Translation |
| <img src="https://flagcdn.com/w20/es.png" width="20"> Spanish | ![100%](https://geps.dev/progress/100?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `es.json` | Credits: [@Alawapr](https://github.com/Alawapr)
| <img src="https://flagcdn.com/w20/fr.png" width="20"> French | ![0%](https://geps.dev/progress/0?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `fr.json` | No Translation |
| <img src="https://flagcdn.com/w20/de.png" width="20"> German | ![1000%](https://geps.dev/progress/100?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `de.json` |
| <img src="https://flagcdn.com/w20/tr.png" width="20"> Turkish | ![100%](https://geps.dev/progress/100?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `tr.json` |
| <img src="https://flagcdn.com/w20/cn.png" width="20"> Chinese (Simplified) | ![70%](https://geps.dev/progress/70?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `cn.json` | (Unofficial Translation) |
| <img src="https://flagcdn.com/w20/jp.png" width="20"> Japanese | ![70%](https://geps.dev/progress/70?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `jp.json` | (Unofficial Translation) |
| <img src="https://flagcdn.com/w20/kr.png" width="20"> Korean | ![0%](https://geps.dev/progress/0?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `kr.json` | No Translation |
| <img src="https://flagcdn.com/w20/vn.png" width="20"> Vietnamese | ![100%](https://geps.dev/progress/100?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `vi.json` | Credits: [@TanNhatCMS](https://github.com/TanNhatCMS) |
| <img src="https://flagcdn.com/w20/nl.png" width="20"> Dutch | ![100%](https://geps.dev/progress/100?dangerColor=e05d44&warningColor=dfb317&successColor=2ea44f) | `nl.json` | Credits: [@devAxri](https://github.com/devAxri)

### ᅠ
## Contributors
<a href="https://github.com/Daturaxoxo/Aurora/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=Daturaxoxo/Aurora&anon=1" />
</a>
