//! What the window picker is for. A click on a panel's zone always opens the
//! picker, so the user chooses which window moves: the panel never takes the
//! focus, so "the window the user was in" is just whatever was last in front,
//! and moving it unasked surprised people. The grid button still opens the
//! grid on that window when it can be moved, since the grid itself is the
//! confirmation step.
use super::{js_trim, OpenWindow};

pub const MAX_ENTRIES: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The target can be moved directly.
    Snap,
    /// The picker should open instead.
    Pick,
    /// The window manager that lists and moves windows isn't running yet.
    Wait,
}

/// For the grid button. `target_snappable` is the foreground window's
/// `snappable` flag, or None when there is no foreground window.
pub fn panel_action(target_snappable: Option<bool>, helper_up: bool) -> Action {
    if !helper_up {
        Action::Wait
    } else if target_snappable == Some(true) {
        Action::Snap
    } else {
        Action::Pick
    }
}

/// A zone clicked in a panel: Pick, or Wait while the helper is down.
/// `grid_target_snappable` is for the window the open grid placer is moving,
/// if it is open: that one was already chosen, so a zone moves it straight
/// away instead.
pub fn zone_click_action(helper_up: bool, grid_target_snappable: Option<bool>) -> Action {
    panel_action(grid_target_snappable, helper_up)
}

/// The window list (top of the z-order first) as picker rows. Anything
/// without a usable handle or title is dropped, each window appears once,
/// titles are trimmed, and the list is capped so the menu stays a menu.
pub fn picker_entries(windows: &[OpenWindow]) -> Vec<OpenWindow> {
    let mut entries: Vec<OpenWindow> = Vec::new();
    for w in windows {
        if w.hwnd == 0 || entries.iter().any(|e| e.hwnd == w.hwnd) {
            continue;
        }
        let title = js_trim(&w.title);
        if title.is_empty() {
            continue;
        }
        entries.push(OpenWindow { title: title.to_owned(), ..w.clone() });
        if entries.len() == MAX_ENTRIES {
            break;
        }
    }
    entries
}

/// The row the picker reported (by handle), or None if it isn't one it was
/// shown - the picker only ever sends back what it was given.
pub fn find_entry(entries: &[OpenWindow], hwnd: i64) -> Option<&OpenWindow> {
    entries.iter().find(|e| e.hwnd == hwnd)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The JS passed a foreground event; only its snappable flag is read.
    fn fg(_kind: &str, snappable: bool) -> Option<bool> {
        Some(snappable)
    }
    fn win(hwnd: i64, exe: &str, title: &str, minimized: bool) -> OpenWindow {
        OpenWindow { hwnd, exe: exe.into(), title: title.into(), minimized }
    }

    #[test]
    fn panel_action_the_grid_button_opens_the_grid_on_a_snappable_window_straight_away() {
        assert_eq!(panel_action(fg("app", true), true), Action::Snap);
    }

    #[test]
    fn zone_click_action_always_opens_the_picker_for_a_panel_zone_even_with_a_snappable_window_in_front() {
        // JS also called it with the argument left out; None covers both.
        assert_eq!(zone_click_action(true, None), Action::Pick);
        assert_eq!(zone_click_action(false, None), Action::Wait);
    }

    #[test]
    fn zone_click_action_moves_the_window_the_open_grid_placer_is_moving() {
        assert_eq!(zone_click_action(true, fg("app", true)), Action::Snap);
        assert_eq!(zone_click_action(true, fg("app", false)), Action::Pick);
        assert_eq!(zone_click_action(false, fg("app", true)), Action::Wait);
    }

    #[test]
    fn panel_action_opens_the_picker_when_there_is_nothing_to_move() {
        assert_eq!(panel_action(fg("own", false), true), Action::Pick); // Launchpad in front
        assert_eq!(panel_action(fg("shell", false), true), Action::Pick); // the desktop
        assert_eq!(panel_action(fg("app", false), true), Action::Pick); // a fixed-size dialog
        assert_eq!(panel_action(None, true), Action::Pick);
    }

    #[test]
    fn panel_action_waits_while_the_helper_is_down_whatever_is_focused() {
        assert_eq!(panel_action(fg("app", true), false), Action::Wait);
        assert_eq!(panel_action(None, false), Action::Wait);
    }

    #[test]
    fn picker_entries_keeps_z_order_and_drops_unusable_duplicate_and_untitled_windows() {
        // Not expressible with a typed list: the JS rows with a text handle
        // ('x'), a null row, a missing exe, and an undefined list.
        let entries = picker_entries(&[
            win(3, "C:\\a.exe", " Notes ", false),
            win(0, "C:\\b.exe", "Zero", false),
            win(3, "C:\\a.exe", "Again", false),
            win(5, "C:\\c.exe", "   ", false),
            win(7, "", "Mail", true),
        ]);
        assert_eq!(entries, [win(3, "C:\\a.exe", "Notes", false), win(7, "", "Mail", true)]);
        assert_eq!(picker_entries(&[]), []);
    }

    #[test]
    fn picker_entries_caps_the_list() {
        let many: Vec<OpenWindow> =
            (0..MAX_ENTRIES + 5).map(|i| win(i as i64 + 1, "a.exe", &format!("W{i}"), false)).collect();
        let entries = picker_entries(&many);
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].title, "W0");
    }

    #[test]
    fn find_entry_only_finds_windows_the_picker_was_shown() {
        let entries = picker_entries(&[win(3, "a.exe", "Notes", false)]);
        assert_eq!(find_entry(&entries, 3).unwrap().title, "Notes");
        assert_eq!(find_entry(&entries, 4), None);
        // Not expressible: findEntry(null, 3); an empty list stands in.
        assert_eq!(find_entry(&[], 3), None);
    }
}
