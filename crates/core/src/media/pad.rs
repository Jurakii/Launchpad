//! Controller input for Media: turns gamepad snapshots into named actions
//! (up, down, left, right, select, back, tabPrev, tabNext, menu, pageUp,
//! pageDown, quick, customize). Ported from renderer/media/pad.js; the
//! browser loop around it (startPads: requestAnimationFrame, focus and
//! capture handling) stays with the UI.
use std::collections::BTreeMap;

/// Button index -> action. Ordered by button, like a JS object's integer keys.
pub type Mapping = BTreeMap<u32, String>;

/// The Gamepad API's standard mapping (Xbox layout). Pads Chromium doesn't
/// recognize report other indices; remapping them comes with the Controller
/// setup screen.
pub fn default_mapping() -> Mapping {
    [
        (0, "select"),    // A
        (1, "back"),      // B
        (2, "quick"),     // X
        (3, "customize"), // Y
        (4, "tabPrev"),   // LB
        (5, "tabNext"),   // RB
        (6, "pageUp"),    // LT
        (7, "pageDown"),  // RT
        (9, "menu"),      // Start
        (12, "up"),
        (13, "down"),
        (14, "left"),
        (15, "right"),
    ]
    .into_iter()
    .map(|(b, a)| (b, a.to_string()))
    .collect()
}

pub const DEFAULT_DEAD_ZONE: f64 = 0.35;
const TRIGGER_PRESS: f64 = 0.5; // analogue triggers count as pressed past this
pub const REPEAT_DELAY_MS: f64 = 400.0;
pub const REPEAT_RATE_MS: f64 = 80.0;
const REPEATS: [&str; 6] = ["up", "down", "left", "right", "pageUp", "pageDown"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Button {
    pub pressed: bool,
    pub value: Option<f64>,
}

/// A snapshot shaped like a Gamepad. `buttons` entries may be missing
/// (None), as a hand-built snapshot's could be.
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub id: String,
    pub index: u32,
    /// Only an explicit `false` counts as disconnected.
    pub connected: Option<bool>,
    pub buttons: Vec<Option<Button>>,
    pub axes: Vec<f64>,
}

/// The left stick as a direction, or None inside the dead zone. The larger
/// axis wins, so a diagonal push moves one way, not two.
pub fn stick_direction(x: f64, y: f64, dead_zone: f64) -> Option<&'static str> {
    if x.hypot(y) < dead_zone {
        return None;
    }
    if x.abs() >= y.abs() {
        return Some(if x < 0.0 { "left" } else { "right" });
    }
    Some(if y < 0.0 { "up" } else { "down" })
}

fn add(held: &mut Vec<String>, action: &str) {
    if !held.iter().any(|a| a == action) {
        held.push(action.to_string());
    }
}

/// Which actions a pad snapshot is holding, in button order then the stick
/// (the insertion order of the JS Set).
pub fn held_actions(pad: &Pad, mapping: &Mapping, dead_zone: f64) -> Vec<String> {
    let mut held = Vec::new();
    for (i, b) in pad.buttons.iter().enumerate() {
        let (Some(action), Some(b)) = (mapping.get(&(i as u32)), b) else { continue };
        if b.pressed || b.value.is_some_and(|v| v >= TRIGGER_PRESS) {
            add(&mut held, action);
        }
    }
    // axes[0] || 0: missing and NaN read as 0
    let axis = |i: usize| pad.axes.get(i).copied().filter(|v| !v.is_nan()).unwrap_or(0.0);
    if let Some(dir) = stick_direction(axis(0), axis(1), dead_zone) {
        add(&mut held, dir);
    }
    held
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assigned {
    pub mapping: Mapping,
    pub swapped: Option<String>,
    pub unmapped: Option<String>,
}

/// Puts `action` on `button` in a copy of `mapping`. The action the button
/// had before moves to the action's old button (a swap), so remapping never
/// leaves select or back without a button; with no old button to swap to,
/// that other action is left unmapped and returned as `unmapped`.
pub fn assign_button(mapping: &Mapping, action: &str, button: u32) -> Assigned {
    let mut next = mapping.clone();
    let old_button = next.iter().find(|(_, a)| *a == action).map(|(b, _)| *b);
    let other = next.get(&button).cloned();
    if let Some(old) = old_button {
        next.remove(&old);
    }
    next.insert(button, action.to_string());
    match other {
        Some(other) if other != action => match old_button {
            Some(old) if old != button => {
                next.insert(old, other.clone());
                Assigned { mapping: next, swapped: Some(other), unmapped: None }
            }
            _ => Assigned { mapping: next, swapped: None, unmapped: Some(other) },
        },
        _ => Assigned { mapping: next, swapped: None, unmapped: None },
    }
}

pub type MappingFor = Box<dyn Fn(&Pad) -> Mapping>;

/// Edge-triggered actions with key-repeat for movement: an action fires
/// when pressed, then (if it repeats) after REPEAT_DELAY_MS and every
/// REPEAT_RATE_MS while held. Several pads act as one, each with its own
/// mapping (`mapping_for(pad)`).
pub struct PadReader {
    holding: Vec<(String, f64)>, // action -> time it should fire next, in insertion order
    map_of: MappingFor,
    zone: f64,
}

impl PadReader {
    /// `mapping_for` wins over `mapping`; with neither, the default mapping.
    pub fn new(mapping: Option<Mapping>, mapping_for: Option<MappingFor>, dead_zone: Option<f64>) -> Self {
        let map_of = mapping_for.unwrap_or_else(|| {
            let m = mapping.unwrap_or_else(default_mapping);
            Box::new(move |_| m.clone())
        });
        PadReader { holding: Vec::new(), map_of, zone: dead_zone.unwrap_or(DEFAULT_DEAD_ZONE) }
    }

    pub fn configure(&mut self, mapping_for: Option<MappingFor>, dead_zone: Option<f64>) {
        if let Some(f) = mapping_for {
            self.map_of = f;
        }
        if let Some(z) = dead_zone {
            self.zone = z;
        }
    }

    /// The actions that fire this frame. `now` is in milliseconds.
    pub fn update(&mut self, pads: &[Option<Pad>], now: f64) -> Vec<String> {
        let mut held = Vec::new();
        for pad in pads.iter().flatten().filter(|p| p.connected != Some(false)) {
            for a in held_actions(pad, &(self.map_of)(pad), self.zone) {
                add(&mut held, &a);
            }
        }
        let mut fired = Vec::new();
        for action in &held {
            match self.holding.iter_mut().find(|(a, _)| a == action) {
                None => {
                    fired.push(action.clone());
                    let next = if REPEATS.contains(&action.as_str()) { now + REPEAT_DELAY_MS } else { f64::INFINITY };
                    self.holding.push((action.clone(), next));
                }
                Some((_, at)) if now >= *at => {
                    fired.push(action.clone());
                    *at = now + REPEAT_RATE_MS;
                }
                Some(_) => {}
            }
        }
        self.holding.retain(|(a, _)| held.contains(a));
        fired
    }

    /// Forget what is held, so buttons still down when input resumes don't
    /// fire (the A that closed a game must not also launch something).
    /// Held buttons are swallowed until released.
    pub fn reset(&mut self, pads: &[Option<Pad>]) {
        self.holding.clear();
        for pad in pads.iter().flatten() {
            for a in held_actions(pad, &(self.map_of)(pad), self.zone) {
                match self.holding.iter_mut().find(|(h, _)| *h == a) {
                    Some((_, at)) => *at = f64::INFINITY,
                    None => self.holding.push((a, f64::INFINITY)),
                }
            }
        }
    }
}

impl Default for PadReader {
    fn default() -> Self {
        PadReader::new(None, None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A pad snapshot shaped like a Gamepad: buttons by index, left stick axes.
    fn snapshot(buttons: &[usize], axes: [f64; 2], triggers: &[(usize, f64)]) -> Pad {
        let mut list: Vec<Option<Button>> = (0..17)
            .map(|i| {
                let on = buttons.contains(&i);
                Some(Button { pressed: on, value: Some(if on { 1.0 } else { 0.0 }) })
            })
            .collect();
        for &(i, value) in triggers {
            list[i] = Some(Button { pressed: false, value: Some(value) });
        }
        Pad { id: String::new(), index: 0, connected: Some(true), buttons: list, axes: axes.to_vec() }
    }

    fn press(buttons: &[usize]) -> Option<Pad> {
        Some(snapshot(buttons, [0.0, 0.0], &[]))
    }

    fn idle() -> Option<Pad> {
        press(&[])
    }

    fn map(pairs: &[(u32, &str)]) -> Mapping {
        pairs.iter().map(|(b, a)| (*b, a.to_string())).collect()
    }

    const NONE: [&str; 0] = [];

    #[test]
    fn stick_direction_nothing_inside_the_dead_zone_the_larger_axis_outside_it() {
        assert_eq!(stick_direction(0.2, 0.2, 0.35), None);
        assert_eq!(stick_direction(0.9, 0.1, 0.35), Some("right"));
        assert_eq!(stick_direction(-0.5, 0.2, 0.35), Some("left"));
        assert_eq!(stick_direction(0.3, -0.8, 0.35), Some("up"));
        assert_eq!(stick_direction(0.1, 0.6, 0.35), Some("down"));
    }

    #[test]
    fn the_default_mapping_follows_the_xbox_layout() {
        let held = |buttons: &[usize]| held_actions(&snapshot(buttons, [0.0, 0.0], &[]), &default_mapping(), 0.35);
        assert_eq!(held(&[0]), ["select"]);
        assert_eq!(held(&[1]), ["back"]);
        assert_eq!(held(&[4, 5]), ["tabPrev", "tabNext"]);
        assert_eq!(held(&[9]), ["menu"]);
        assert_eq!(held(&[12, 15]), ["up", "right"]);
        assert_eq!(held(&[16]), NONE, "the Guide button is not mapped");
    }

    #[test]
    fn analogue_triggers_count_as_pressed_past_half_way() {
        let held = |value: f64| held_actions(&snapshot(&[], [0.0, 0.0], &[(7, value)]), &default_mapping(), 0.35);
        assert_eq!(held(0.3), NONE);
        assert_eq!(held(0.8), ["pageDown"]);
    }

    #[test]
    fn a_press_fires_once_not_every_frame_it_is_held() {
        let mut reader = PadReader::default();
        let a = [press(&[0])];
        assert_eq!(reader.update(&a, 0.0), ["select"]);
        assert_eq!(reader.update(&a, 16.0), NONE);
        assert_eq!(reader.update(&a, 2000.0), NONE, "select never repeats");
        assert_eq!(reader.update(&[idle()], 2016.0), NONE);
        assert_eq!(reader.update(&a, 2032.0), ["select"], "fires again after release");
    }

    #[test]
    fn held_movement_repeats_after_the_delay_then_at_the_repeat_rate() {
        let mut reader = PadReader::default();
        let down = [press(&[13])];
        let (delay, rate) = (REPEAT_DELAY_MS, REPEAT_RATE_MS);
        assert_eq!(reader.update(&down, 0.0), ["down"]);
        assert_eq!(reader.update(&down, delay - 1.0), NONE);
        assert_eq!(reader.update(&down, delay), ["down"]);
        assert_eq!(reader.update(&down, delay + rate - 1.0), NONE);
        assert_eq!(reader.update(&down, delay + rate), ["down"]);
    }

    #[test]
    fn the_stick_moves_like_the_d_pad_and_respects_a_custom_dead_zone() {
        let mut reader = PadReader::new(None, None, Some(0.6));
        assert_eq!(reader.update(&[Some(snapshot(&[], [0.5, 0.0], &[]))], 0.0), NONE);
        assert_eq!(reader.update(&[Some(snapshot(&[], [0.7, 0.0], &[]))], 16.0), ["right"]);
    }

    #[test]
    fn reset_swallows_buttons_still_held_until_they_are_released() {
        let mut reader = PadReader::default();
        let a = [press(&[0])];
        reader.reset(&a);
        assert_eq!(reader.update(&a, 0.0), NONE);
        assert_eq!(reader.update(&a, 5000.0), NONE);
        reader.update(&[idle()], 5016.0);
        assert_eq!(reader.update(&a, 5032.0), ["select"]);
    }

    #[test]
    fn two_pads_act_as_one_and_disconnected_pads_are_ignored() {
        let mut reader = PadReader::default();
        let mut fired = reader.update(&[press(&[0]), press(&[0, 1])], 0.0);
        fired.sort();
        assert_eq!(fired, ["back", "select"]);
        let mut gone = snapshot(&[9], [0.0, 0.0], &[]);
        gone.connected = Some(false);
        assert_eq!(reader.update(&[Some(gone), None], 16.0), NONE);
    }

    #[test]
    fn assign_button_swaps_with_the_action_that_had_the_button() {
        let start = map(&[(0, "select"), (1, "back"), (2, "quick")]);
        let out = assign_button(&start, "select", 1);
        assert_eq!(out.mapping, map(&[(0, "back"), (1, "select"), (2, "quick")]));
        assert_eq!((out.swapped.as_deref(), out.unmapped.as_deref()), (Some("back"), None));
        assert_eq!(start, map(&[(0, "select"), (1, "back"), (2, "quick")]), "input not mutated");
    }

    #[test]
    fn assign_button_onto_a_free_button_just_moves_the_action() {
        let out = assign_button(&map(&[(0, "select"), (1, "back")]), "back", 7);
        assert_eq!(out.mapping, map(&[(0, "select"), (7, "back")]));
        assert_eq!((out.swapped, out.unmapped), (None, None));
    }

    #[test]
    fn assign_button_for_an_unmapped_action_takes_the_button_from_its_owner() {
        let out = assign_button(&map(&[(0, "select"), (1, "back"), (9, "menu")]), "quick", 9);
        assert_eq!(out.mapping, map(&[(0, "select"), (1, "back"), (9, "quick")]));
        assert_eq!(out.unmapped.as_deref(), Some("menu"));
    }

    #[test]
    fn each_pad_uses_its_own_mapping_and_configure_changes_it_live() {
        let mut reader = PadReader::new(
            None,
            Some(Box::new(|p: &Pad| if p.id == "odd" { map(&[(1, "select")]) } else { default_mapping() })),
            None,
        );
        let with_id = |id: &str| Some(Pad { id: id.into(), ..snapshot(&[1], [0.0, 0.0], &[]) });
        assert_eq!(reader.update(&[with_id("odd")], 0.0), ["select"]);
        assert_eq!(reader.update(&[idle()], 16.0), NONE);
        assert_eq!(reader.update(&[with_id("std")], 32.0), ["back"]);
        reader.update(&[idle()], 48.0);
        reader.configure(Some(Box::new(|_: &Pad| map(&[(1, "menu")]))), Some(0.9));
        assert_eq!(reader.update(&[with_id("std")], 64.0), ["menu"]);
        assert_eq!(reader.update(&[Some(snapshot(&[], [0.8, 0.0], &[]))], 80.0), NONE, "new dead zone applies");
    }

    #[test]
    fn a_custom_mapping_replaces_the_default() {
        let mut reader = PadReader::new(Some(map(&[(1, "select"), (0, "back")])), None, None);
        assert_eq!(reader.update(&[press(&[1])], 0.0), ["select"]);
    }
}
