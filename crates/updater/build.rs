fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set("CompanyName", "Aurora Team");
        res.set("FileDescription", "Aurora Updater");
        res.set("ProductName", "Aurora");
        res.set("OriginalFilename", "updater.exe");
        res.set("LegalCopyright", "Copyright (c) 2026 Daturaxoxo");
        res.compile().unwrap();
    }
}
