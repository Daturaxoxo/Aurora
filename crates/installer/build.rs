#[cfg(target_os = "windows")]
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#;

fn main() {
    slint_build::compile("./frontend/main.slint").unwrap();
    slint_build::compile("./frontend/uninstall.slint").unwrap();

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../production/icons/logo.ico");
        res.set_manifest(MANIFEST);
        res.compile().unwrap();
    }
}
