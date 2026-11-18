//! Controllers through XInput, as snapshots shaped like the browser's Gamepad
//! (core::media::pad reads them): button numbers follow the Gamepad API's
//! standard mapping, so the default map and the saved ones mean the same.
//! Polled only while Media is in front (1.2's requestAnimationFrame loop).
use launchpad_core::media::pad::{Button, Pad};
use windows::Win32::UI::Input::XboxController::*;

pub const SLOTS: u32 = 4;
// XInputGetState on an empty slot is slow, so those are looked at about once a second.
const EMPTY_EVERY: u32 = 60;

// XInput's buttons, by their standard Gamepad number.
const BUTTONS: [(XINPUT_GAMEPAD_BUTTON_FLAGS, usize); 14] = [
    (XINPUT_GAMEPAD_A, 0),
    (XINPUT_GAMEPAD_B, 1),
    (XINPUT_GAMEPAD_X, 2),
    (XINPUT_GAMEPAD_Y, 3),
    (XINPUT_GAMEPAD_LEFT_SHOULDER, 4),
    (XINPUT_GAMEPAD_RIGHT_SHOULDER, 5),
    (XINPUT_GAMEPAD_BACK, 8),
    (XINPUT_GAMEPAD_START, 9),
    (XINPUT_GAMEPAD_LEFT_THUMB, 10),
    (XINPUT_GAMEPAD_RIGHT_THUMB, 11),
    (XINPUT_GAMEPAD_DPAD_UP, 12),
    (XINPUT_GAMEPAD_DPAD_DOWN, 13),
    (XINPUT_GAMEPAD_DPAD_LEFT, 14),
    (XINPUT_GAMEPAD_DPAD_RIGHT, 15),
];

/// The id a pad's saved map is kept under (Chromium's ids don't carry over).
pub fn pad_id(slot: u32) -> String {
    format!("XInput controller {}", slot + 1)
}

/// What Chromium's Gamepad gave for an XInput pad, from its state.
pub fn snapshot(slot: u32, g: &XINPUT_GAMEPAD) -> Pad {
    let mut buttons: Vec<Option<Button>> = vec![Some(Button::default()); 17];
    for (flag, i) in BUTTONS {
        let on = g.wButtons.0 & flag.0 != 0;
        buttons[i] = Some(Button { pressed: on, value: Some(if on { 1.0 } else { 0.0 }) });
    }
    for (i, t) in [(6, g.bLeftTrigger), (7, g.bRightTrigger)] {
        let v = f64::from(t) / 255.0;
        buttons[i] = Some(Button { pressed: t > XINPUT_GAMEPAD_TRIGGER_THRESHOLD.0 as u8, value: Some(v) });
    }
    // Gamepad's y axis points down, XInput's up.
    let axis = |v: i16| (f64::from(v) / 32767.0).clamp(-1.0, 1.0);
    Pad { id: pad_id(slot), index: slot, connected: Some(true), buttons, axes: vec![axis(g.sThumbLX), -axis(g.sThumbLY), axis(g.sThumbRX), -axis(g.sThumbRY)] }
}

#[derive(Default)]
pub struct Pads {
    connected: [bool; SLOTS as usize],
    frame: u32,
}

impl Pads {
    /// One snapshot per slot (None for an empty one).
    pub fn read(&mut self) -> Vec<Option<Pad>> {
        self.frame = self.frame.wrapping_add(1);
        let probe = self.frame % EMPTY_EVERY == 1;
        (0..SLOTS)
            .map(|slot| {
                if !self.connected[slot as usize] && !probe {
                    return None;
                }
                let mut s = XINPUT_STATE::default();
                let ok = unsafe { XInputGetState(slot, &mut s) } == 0;
                self.connected[slot as usize] = ok;
                ok.then(|| snapshot(slot, &s.Gamepad))
            })
            .collect()
    }
}

/// Button numbers held on a pad (for Controller setup's capture).
pub fn held(pad: &Pad) -> Vec<u32> {
    pad.buttons
        .iter()
        .enumerate()
        .filter(|(_, b)| b.as_ref().is_some_and(|b| b.pressed || b.value.is_some_and(|v| v >= 0.5)))
        .map(|(i, _)| i as u32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use launchpad_core::media::pad::{default_mapping, held_actions};

    #[test]
    fn xinput_buttons_land_on_the_standard_gamepad_numbers() {
        let g = XINPUT_GAMEPAD { wButtons: XINPUT_GAMEPAD_A | XINPUT_GAMEPAD_DPAD_RIGHT, bRightTrigger: 255, ..Default::default() };
        let p = snapshot(0, &g);
        assert_eq!(held(&p), [0, 7, 15]);
        assert_eq!(held_actions(&p, &default_mapping(), 0.35), ["select", "pageDown", "right"]);
    }

    #[test]
    fn stick_up_is_up() {
        let g = XINPUT_GAMEPAD { sThumbLY: 30000, ..Default::default() };
        assert_eq!(held_actions(&snapshot(1, &g), &default_mapping(), 0.35), ["up"]);
    }
}
