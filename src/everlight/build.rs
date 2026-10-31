use std::env;
use std::path::PathBuf;
fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let part = |name: &str| env::var(name).unwrap();
    let numeric = format!(
        "{},{},{},0",
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH")
    );

    let rc = format!(
        r#"#include <windows.h>

VS_VERSION_INFO VERSIONINFO
 FILEVERSION    {numeric}
 PRODUCTVERSION {numeric}
 FILEFLAGSMASK  0x3fL
 FILEFLAGS      0x0L
 FILEOS         VOS_NT_WINDOWS32
 FILETYPE       VFT_DLL
 FILESUBTYPE    VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName",      "AuroraTeam"
            VALUE "FileDescription",  "Aurora's Engine"
            VALUE "FileVersion",      "{version}"
            VALUE "InternalName",     "Everlight"
            VALUE "LegalCopyright",   "Copyright (C) 2026 Aurora Launcher"
            VALUE "LegalTrademarks",  "Aurora Launcher, Everlight"
            VALUE "OriginalFilename", "Everlight.asi"
            VALUE "ProductName",      "Everlight"
            VALUE "ProductVersion",   "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );

    let rc_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("everlight.rc");
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile_for_cdylib(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
