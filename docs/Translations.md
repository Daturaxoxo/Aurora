# How to add translations to Aurora
> [!NOTE]
> This tutorial skips the very basics of using GitHub. If you don't know how to create pull requests, please look up a guide online.

## Already Existing Language
If your language already exists but you'd like to fix any issues with the translation. You are only required to edit the specific section of the langs.json file.
## Unsupported Language / Adding a new language
If your language doesn't exist and isn't translated in Aurora yet, create support for it by doing the following:
> [!IMPORTANT]
> **Step 1:** Edit `src\config_manager.py`, add your language to the `LANG_CODES` array.
> 
> **Step 2:** Edit `src\frontend\classes\settings.py`, find the language dropdown (named `self._lang_box`) and add your language there.
> 
> **Step 3:** Add your own language's section to `Lang\langs.json`. Translate the items in there.
>
> **Step 4:** Create a pull request with a title like `"Added language support for {X}"`
