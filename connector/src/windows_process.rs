pub fn configure_background(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
}
