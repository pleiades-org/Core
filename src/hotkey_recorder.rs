use std::time::Duration;

#[cfg(target_os = "windows")]
pub fn record_hotkey_combo(timeout: Duration) -> Option<String> {
    use std::thread;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RMENU, VK_RSHIFT, VK_SHIFT,
    };

    let deadline = std::time::Instant::now() + timeout;

    loop {
        if std::time::Instant::now() >= deadline {
            return None;
        }

        if let Some(combo) = detect_hotkey_combo(&[
            (VK_CONTROL.0 as i32, "Ctrl"),
            (VK_SHIFT.0 as i32, "Shift"),
            (VK_MENU.0 as i32, "Alt"),
            (VK_LSHIFT.0 as i32, "Shift"),
            (VK_RSHIFT.0 as i32, "Shift"),
            (VK_LMENU.0 as i32, "Alt"),
            (VK_RMENU.0 as i32, "Alt"),
        ]) {
            return Some(combo);
        }

        thread::sleep(Duration::from_millis(35));
    }
}

#[cfg(target_os = "windows")]
fn detect_hotkey_combo(modifier_keys: &[(i32, &str)]) -> Option<String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VIRTUAL_KEY, VK_A, VK_BACK, VK_DELETE, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_INSERT,
        VK_LWIN, VK_NEXT, VK_NUMPAD0, VK_NUMPAD9, VK_PRIOR, VK_RWIN, VK_SPACE, VK_TAB, VK_Z,
    };

    let mut modifiers = Vec::new();
    for (virtual_key, label) in modifier_keys {
        if is_key_down(*virtual_key) && !modifiers.iter().any(|(_, existing)| existing == label) {
            modifiers.push((*virtual_key, *label));
        }
    }

    if modifiers.is_empty() {
        return None;
    }

    let main_key = (VK_A.0..=VK_Z.0)
        .chain(VK_F1.0..=VK_F24.0)
        .chain(VK_NUMPAD0.0..=VK_NUMPAD9.0)
        .chain([
            VK_SPACE.0,
            VK_TAB.0,
            VK_ESCAPE.0,
            VK_BACK.0,
            VK_DELETE.0,
            VK_INSERT.0,
            VK_HOME.0,
            VK_END.0,
            VK_PRIOR.0,
            VK_NEXT.0,
        ])
        .find(|virtual_key| is_key_down(i32::from(*virtual_key)))?;

    if main_key == VK_LWIN.0 || main_key == VK_RWIN.0 {
        return None;
    }

    let mut parts = modifiers
        .into_iter()
        .map(|(_, label)| label.to_string())
        .collect::<Vec<_>>();
    parts.sort_by_key(|label| match label.as_str() {
        "Ctrl" => 0,
        "Alt" => 1,
        "Shift" => 2,
        _ => 3,
    });
    parts.push(virtual_key_name(VIRTUAL_KEY(main_key))?);
    Some(parts.join("+"))
}

#[cfg(target_os = "windows")]
fn is_key_down(virtual_key: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { GetAsyncKeyState(virtual_key) as u16 & 0x8000u16 != 0 }
}

#[cfg(target_os = "windows")]
fn virtual_key_name(virtual_key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> Option<String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_A, VK_BACK, VK_DELETE, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT, VK_NUMPAD0,
        VK_NUMPAD9, VK_SPACE, VK_TAB, VK_Z,
    };

    let code = virtual_key.0;
    if (VK_A.0..=VK_Z.0).contains(&code) {
        return char::from_u32((code - VK_A.0) as u32 + u32::from(b'A'))
            .map(|character| character.to_string());
    }
    if (VK_F1.0..=VK_F1.0 + 23).contains(&code) {
        return Some(format!("F{}", code - VK_F1.0 + 1));
    }
    if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&code) {
        return Some(format!("Num{}", code - VK_NUMPAD0.0));
    }

    Some(match virtual_key {
        VK_SPACE => "Space".to_string(),
        VK_TAB => "Tab".to_string(),
        VK_ESCAPE => "Escape".to_string(),
        VK_BACK => "Backspace".to_string(),
        VK_DELETE => "Delete".to_string(),
        VK_INSERT => "Insert".to_string(),
        VK_HOME => "Home".to_string(),
        VK_END => "End".to_string(),
        _ => return None,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn record_hotkey_combo(_timeout: Duration) -> Option<String> {
    None
}