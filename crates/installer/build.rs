#[cfg(target_os = "windows")]
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10/11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <!-- Windows 8.1 -->
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <!-- Windows 8 -->
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <!-- Windows 7 -->
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    </application>
  </compatibility>
</assembly>"#;

fn main() {
    slint_build::compile("./frontend/main.slint").unwrap();
    slint_build::compile("./frontend/uninstall.slint").unwrap();

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../production/icons/logo.ico");
        res.set_manifest(MANIFEST);

        // A single version resource is linked into every binary this crate
        // produces, so these values have to describe both AuroraInstaller.exe
        // and AuroraUninstaller.exe. OriginalFilename and InternalName are
        // deliberately left unset rather than being wrong on one of the two:
        // a filename that disagrees with the binary it is attached to reads as
        // a tampered file to reputation scanners.
        res.set("CompanyName", "Daturaxoxo");
        res.set("ProductName", "Aurora");
        res.set("FileDescription", "Aurora Setup");
        res.set("LegalCopyright", "Copyright (c) 2026 Daturaxoxo");

        res.compile().unwrap();
    }
}
