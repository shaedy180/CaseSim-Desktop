#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Home,
    Account,
    Controls,
    Fullscreen,
    ResetZoom,
}

// Native key input only. Browser DOM events and remote IPC never reach this API.
#[cfg(any(windows, test))]
pub fn resolve(key: u32, control: bool, shift: bool, alt: bool, meta: bool) -> Option<Action> {
    if alt || meta {
        return None;
    }
    match (key, control, shift) {
        (0x7a, false, false) => Some(Action::Fullscreen), // F11
        (0x30 | 0x60, true, false) => Some(Action::ResetZoom),
        (0x48, true, true) => Some(Action::Home),
        (0x41, true, true) => Some(Action::Account),
        (0x44, true, true) => Some(Action::Controls),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_existing_and_new_native_shortcuts_resolve_without_a_menu() {
        for (key, control, shift, action) in [
            (0x7a, false, false, Action::Fullscreen),
            (0x30, true, false, Action::ResetZoom),
            (0x60, true, false, Action::ResetZoom),
            (0x48, true, true, Action::Home),
            (0x41, true, true, Action::Account),
            (0x44, true, true, Action::Controls),
        ] {
            assert_eq!(resolve(key, control, shift, false, false), Some(action));
        }
    }
    #[test]
    fn plain_input_altgr_and_windows_shortcuts_remain_untouched() {
        for key in [0x30, 0x41, 0x44, 0x48] {
            assert_eq!(resolve(key, false, false, false, false), None);
        }
        for (control, shift, alt, meta) in [
            (true, true, true, false),
            (true, true, false, true),
            (false, true, false, false),
            (true, false, false, false),
        ] {
            assert_eq!(resolve(0x44, control, shift, alt, meta), None);
        }
    }
}
