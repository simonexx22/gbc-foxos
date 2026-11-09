//! Keyboard-to-button mapping.

use gbc_core::Button;
use winit::keyboard::PhysicalKey;

use crate::config::GameBoyControls;

/// Maps a physical key to a Game Boy button using the active controls configuration.
pub fn button_for_key(key: PhysicalKey, controls: &GameBoyControls) -> Option<Button> {
    let PhysicalKey::Code(code) = key else {
        return None;
    };

    if code == controls.up {
        Some(Button::Up)
    } else if code == controls.down {
        Some(Button::Down)
    } else if code == controls.left {
        Some(Button::Left)
    } else if code == controls.right {
        Some(Button::Right)
    } else if code == controls.a {
        Some(Button::A)
    } else if code == controls.b {
        Some(Button::B)
    } else if code == controls.start {
        Some(Button::Start)
    } else if code == controls.select {
        Some(Button::Select)
    } else {
        None
    }
}
