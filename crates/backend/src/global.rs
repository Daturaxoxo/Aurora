// Main controller of the engine
// basically contains all constants and other important stuff

pub struct Plugins {
    pub nte: &'static [&'static str],
    pub sp: &'static [&'static str],
}

pub const PLUGINS: Plugins = Plugins {
    nte: &["chksum.asi", "ipc.asi"],
    sp: &[],
};
