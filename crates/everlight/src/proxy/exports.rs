macro_rules! proxy_exports {
    ($($name:ident),+ $(,)?) => {
        pub const COUNT: usize = [$(stringify!($name)),+].len();
        mod real {
            $( pub static mut $name: usize = 0; )+
        }
        $(
            #[unsafe(naked)]
            #[unsafe(export_name = stringify!($name))]
            pub extern "system" fn $name() {
                core::arch::naked_asm!("jmp qword ptr [rip + {slot}]", slot = sym real::$name);
            }
        )+
        pub unsafe fn fill(module: windows_sys::Win32::Foundation::HMODULE) -> usize {
            use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
            let mut found = 0;
            $(
                let addr = unsafe {
                    GetProcAddress(module, concat!(stringify!($name), "\0").as_ptr())
                };
                if let Some(f) = addr {
                    unsafe { real::$name = f as usize };
                    found += 1;
                }
            )+
            found
        }
    };
}
