#[cfg(target_os = "windows")]
pub fn is_workstation_locked() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, SwitchDesktop, DESKTOP_SWITCHDESKTOP,
    };

    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if desktop.is_null() {
            return true;
        }
        let unlocked = SwitchDesktop(desktop) != 0;
        CloseDesktop(desktop);
        !unlocked
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_workstation_locked() -> bool {
    false
}
