#![allow(non_snake_case, non_upper_case_globals)]
pub const DLL_NAME: &str = "xinput1_4.dll";
#[macro_use]
#[path = "exports.rs"]
mod exports;
proxy_exports! {
    XInputEnable,
    XInputGetAudioDeviceIds,
    XInputGetBatteryInformation,
    XInputGetCapabilities,
    XInputGetKeystroke,
    XInputGetState,
    XInputSetState,
}
