<p align="center">
  <img src="https://host.getaurora.moe/assets/web/embed.png" alt="ᅠ" />
</p>
<div align="center">
  
# Aurora Launcher
輕量化動漫遊戲 MOD 平台
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
  <strong>English</strong> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.cn.md">中文</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.tw.md">繁體中文</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.jp.md">日本語</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.tr.md">Türkçe</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/main/README.es.md">Español</a>
</p>
<br></br>
Aurora 是一款專為 Unreal Engine 動漫遊戲打造的輕量化 MOD 平台，讓您自由載入 Unreal Engine 5 的 PAK MOD、Lua 腳本與藍圖。
透過簡潔的介面與超簡單的設定流程，輕鬆開始為喜愛的遊戲安裝 MOD。
<br></br>

輕鬆將 `.pak` 角色模型 MOD 與 `.asi` DLL MOD 載入至遊戲中，同時也支援 Lua 腳本。

Aurora 以 [Rust](https://rust-lang.org) 開發，並使用 [Slint](https://slint.dev) 渲染介面。
<br></br>

> [!NOTE]
> 由於我們的應用程式未經數位簽章，Windows Defender 在首次啟動時可能會對 Aurora 產生誤判。我們目前不支援微軟的 [Smart App Control](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview) 系統，因此您在首次執行 Aurora 時，極有可能會遭到該系統阻擋。
>
> 了解如何停用 Smart App Control：[點我查看！](https://docs.getaurora.moe/hidden/guides/smart-app-control)
<br>
</br>

> [!IMPORTANT]
> Aurora 不支援，且未來也不會支援 `.ini` MOD。這類 MOD 是透過如 3DMigoto 或 XXMI 等 D3D11 Hook 專案來載入。要支援它們幾乎是不可能的，因為 Aurora 與 3DMigoto 的底層架構截然不同。這是一款 Unreal Engine PAK 載入器，而非 DirectX Hook 工具。
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=xfE5l4OXJWrc&format=png&color=000000" height="24" alt=""> 功能特色
</h2>

<table>
<tr>
<td width="33%"><b>輕鬆設定</b><br>具備自動偵測遊戲路徑的引導安裝程式。</td>
<td width="33%"><b>MOD 管理器</b><br>支援一鍵切換、分組、篩選及批次編輯您安裝的所有內容。</td>
<td width="33%"><b>自訂引擎</b><br>我們的 Everlight 引擎專為穩定性與高效能打造。並支援自訂啟動參數。</td>
</tr>
<tr>
<td><b>Lua 腳本</b><br>在啟動器中編寫腳本，並於遊戲內載入。</td>
<td><b>附加元件管理器</b><br>可依需求自由安裝 QoL 附加元件，預設不臃腫。</td>
<td><b>螢幕截圖管理器</b><br>瀏覽您在遊戲中拍攝的截圖。</td>
</tr>
</table>

**支援所有遊戲版本** - 國際、台灣和中國伺服器，在原生啟動器（完美世界）、Steam 和 Epic Games 版本，Aurora 都沒問題。

### 深入 MOD 管理器

<table>
<tr>
<td width="33%"><b>切換</b><br>一鍵啟用或停用</td>
<td width="33%"><b>重新命名</b><br>更改磁碟上的 MOD 名稱</td>
<td width="33%"><b>刪除</b><br>一鍵刪除，並附帶確認提示</td>
</tr>
<tr>
<td><b>搜尋</b><br>透過名稱尋找已安裝的 MOD</td>
<td><b>篩選</b><br>依據啟用狀態、作者或角色進行篩選</td>
<td><b>圖示</b><br>使用內建圖庫，或自訂圖片</td>
</tr>
<tr>
<td><b>檢視模式</b><br>支援清單與網格佈局</td>
<td><b>群組管理</b><br>可折疊的群組，支援拖放操作</td>
<td><b>批量操作</b><br>一次切換、重新命名或刪除多個 MOD</td>
</tr>
<tr>
<td><b>MOD 自動更新</b><br>從 GameBanana 安裝的 MOD 具備版本追蹤功能</td>
<td><b>不相容通知</b><br>有不相容的 MOD 時，Aurora 會通知</td>
<td><b>重新啟動標記</b><br>標示出需「重新啟動遊戲」才會生效的 MOD</td>
</tr>
</table>
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=gXoJoyTtYXFg&format=png&color=ffffff" height="24" alt=""> Windows 安裝
</h2>

### 安裝程式
安裝 Aurora 最簡單且推薦的方式。
1. 從 [latest release](https://github.com/Daturaxoxo/Aurora/releases/latest) 下載 Windows 安裝程式。
2. 執行安裝程式並完成相關設定。
3. 安裝完成後，Aurora 將會自動啟動。

### 免安裝版
1. 從 [latest release](https://github.com/Daturaxoxo/Aurora/releases/latest) 下載 Windows 免安裝版。
2. 將 ZIP 壓縮檔解壓縮至您想存放 Aurora 的資料夾。
3. 執行 Aurora.exe
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=100&id=17842&format=png&color=000000" height="24" alt=""> Linux 安裝
</h2>

> [!NOTE]
> 儘管 Aurora 提供原生 Linux 建置版本，您仍必須使用 DW-Proton 來執行遊戲，以確保遊戲能正常開啟。我們建議使用 `DW-Proton-10.0-26`，而非最新版本的 DW-Proton。

> [!IMPORTANT]
> 在 root 或 sudo 權限下，Aurora 將無法正常運作。如果您的遊戲安裝在需要 root 權限才能存取的資料夾中，建議您將其移動至其他位置。

1. 從 [latest release](https://github.com/Daturaxoxo/Aurora/releases/latest) 下載 Linux 免安裝版。
2. 將 ZIP 壓縮檔解壓縮至您想存放 Aurora 的資料夾。
3. 啟動 Aurora.AppImage
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=keI1M862UTP2&format=png&color=000000" height="24" alt=""> 從原始碼建置
</h2>

> [!IMPORTANT]
> 為了自行建置應用程式，您需要以下工具：[Rust Programming Language](https://rust-lang.org)

> [!WARNING]
> 在建置之前，請先查看我們的 [license](https://github.com/Daturaxoxo/Aurora/blob/main/LICENSE)！


1. 下載本專案的 ZIP 原始碼。（點擊 **Code 按鈕 > "Download ZIP"**）
2. 將原始碼解壓縮至您想要的目標路徑。
3. 在專案的根目錄中以系統管理員身分開啟命令提示字元。
4. 執行 `cargo run`
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=114474&format=png&color=000000" height="24" alt=""> MOD 創作者工具
</h2>

Aurora 提供多種工具供 MOD 創作者使用，包括偵錯視窗與按鍵綁定工具。此外，Aurora 也支援把 metadata 檔案 (`mod.json`) 放入您的 MOD 中，讓我們的 MOD 管理器顯示更多關於您 MOD 的詳細資訊。例如：MOD 版本、圖示（可從內建圖示中挑選，或使用外部網址設定自訂圖示）、作者與支援連結。

了解如何 [建立顯示設定檔](https://docs.getaurora.moe/mod-authors/displaying-your-mod)
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=31016&format=png&color=000000" height="24" alt=""> 翻譯 Aurora
</h2>

我們感謝任何關於翻譯的協助。如果您想為我們的應用程式、網站，甚至這份 README 檔案的翻譯貢獻心力，歡迎參閱我們的翻譯說明文件：

了解如何 [貢獻 Aurora 的翻譯](https://docs.getaurora.moe/translations/translation-status)
