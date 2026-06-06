> [!IMPORTANT]
> This tutorial is for mod creators. If you are someone who doesn't create mods, you should skip this as its not important for you.
### ᅠ
# Why Display Files?
Aurora Display files allow your mod to stand-out in the mod manager, put support links in your mod that when clicked will open your page, display custom information, etc.

They are particularly useful for actually showing what you want to be shown in the mod manager.

> [!NOTE]
> **Before (no mod.json):**
> 
> <a href="https://imgbb.com/"><img src="https://i.ibb.co/b9QsnWM/image.png" alt="image" border="0"></a>
>
>
> **After (with mod.json):**
> 
> <a href="https://ibb.co/VpJNvHdj"><img src="https://i.ibb.co/Z1GJhH3L/image.png" alt="image" border="0"></a>
> <a href="https://imgbb.com/"><img src="https://i.ibb.co/s9mJDJYP/image.png" alt="image" border="0"></a>
### ᅠ
---
### ᅠ
# Adding a Display File to Your Mod
To get your mod recognized properly, you need to add a configuration file. Follow these steps:

### ![](https://img.shields.io/badge/Step_1-Create_the_Configuration_File-7F77DD?style=flat-square)
Create a new file named `mod.json` in the **root (parent) folder** of your mod.

> [!WARNING]
> The `mod.json` file **must** be placed in the parent folder. If it is placed inside a sub-folder, Aurora will not detect it.

**Correct File Hierarchy:**
```yaml
Example Mod Folder Name/
  ├── mod.json                          ◀ CORRECT: File is in the root folder
  └── Mod Skins Folder/
      ├── Example_Mod_File_P.pak
      ├── Example_Mod_File_P.utoc
      ├── Example_Mod_File_P.ucas
      └── mod.json                      ◀ INCORRECT: File isn't on the root folder, Aurora won't detect it.
```
### ᅠ
### ![](https://img.shields.io/badge/Step_2-Edit_Contents-7F77DD?style=flat-square)
Add the following to the newly created file:
```json
{
    "Name": "Example Mod Name",
    "Version": "1.0.0",
    "Author": "Example Author Name",
    "Icon": "CharacterName",
    "Optionals": {
        "Support Link": "https://patreon.com/example"
    }
}
```
For more information about each field, [Click Me!](https://github.com/Daturaxoxo/Aurora/blob/main/docs/ModDisplayFile.md#modjson-documentation)
### ᅠ
### ![](https://img.shields.io/badge/Step_3-Save_and_Test-7F77DD?style=flat-square)
Save the file and test it by refreshing the mod manager in Aurora

If done correctly, your mod will have custom entries. Cheers!
### ᅠ
---
### ᅠ
# Mod.json Documentation

| Field Key | Description | Example
| :--- | :--- | :---: |
| 📛 `Name` | A custom display name for the mod | `"Example Mod Display Name"`
| 🔢 `Version` | The version of the mod | `"v1.7"`
| 👤 `Author` | The name of the mod creator | `"Example Author Name"`
| 🖼️ `Icon` | Set a preset character icon | [All Allowed Characters](https://github.com/Daturaxoxo/Aurora/blob/main/docs/ModDisplayFile.md#allowed-character-names-for-icon-field)
| 🔗 `Support Link` | Set a support link for the mod | `"https://gamebanana.com/mods/123456"`
### ᅠ
---
### ᅠ
# Allowed Character Names for "Icon" Field
| Character Name | Character Icon
| :--- | :--- |
| MC | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/mc.png?raw=true" width="80"> |
| Zero | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/zero.png?raw=true" width="80"> |
| Nanally | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/nanally.png?raw=true" width="80"> |
| Daffodill | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/daffodill.png?raw=true" width="80"> |
| Sakiri | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/sakiri.png?raw=true" width="80"> |
| Adler | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/adler.png?raw=true" width="80"> |
| Edgar | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/edgar.png?raw=true" width="80"> |
| Fadia | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/fadia.png?raw=true" width="80"> |
| Haniel | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/haniel.png?raw=true" width="80"> |
| Hathor | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/hathor.png?raw=true" width="80"> |
| Jiuyuan | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/jiuyuan.png?raw=true" width="80"> |
| Hotori | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/hotori.png?raw=true" width="80"> |
| Mint | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/mint.png?raw=true" width="80"> |
| Lacrimosa | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/lacrimosa.png?raw=true" width="80"> |
| Blackbird | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/blackbird.png?raw=true" width="80"> |
| Baicang | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/baicang.png?raw=true" width="80"> |
| Aurelia | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/aurelia.png?raw=true" width="80"> |
| Skia | <img src="https://github.com/Daturaxoxo/Aurora/blob/main/Bin/Assets/ModImages/skia.png?raw=true" width="80"> |
