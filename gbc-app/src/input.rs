//! Keyboard-to-button mapping.

use gbc_core::Button;
use winit::keyboard::{KeyCode, PhysicalKey};

/// Maps a physical key to a Game Boy button.
///
/// Both arrow keys and WASD are accepted for the D-pad. `Z`/`J` is A,
/// `X`/`K` is B, `Enter` is Start, and `Backspace` is Select.
pub fn button_for_key(key: PhysicalKey) -> Option<Button> {
    let PhysicalKey::Code(code) = key else {
        return None;
    };

    Some(match code {
        KeyCode::ArrowRight | KeyCode::KeyD => Button::Right,
        KeyCode::ArrowLeft | KeyCode::KeyA => Button::Left,
        KeyCode::ArrowUp | KeyCode::KeyW => Button::Up,
        KeyCode::ArrowDown | KeyCode::KeyS => Button::Down,
        KeyCode::KeyZ | KeyCode::KeyJ => Button::A,
        KeyCode::KeyX | KeyCode::KeyK => Button::B,
        KeyCode::Enter => Button::Start,
        KeyCode::Backspace => Button::Select,
        _ => return None,
    })
}
