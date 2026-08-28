fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=oneclick.manifest");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../production/icons/logo.ico");
        res.set_manifest_file("oneclick.manifest");
        res.set("CompanyName", "Aurora Team");
        res.set("FileDescription", "Aurora 1-Click Handler");
        res.set("ProductName", "Aurora");
        res.set("OriginalFilename", "oneclick.exe");
        res.set("LegalCopyright", "Copyright (c) 2026 Daturaxoxo");
        res.compile().unwrap();
    }
}
