//! Whether a fullscreen app has taken over a display, decided from the
//! window manager's fullscreen walk. Rects are physical pixels.
use super::Rect;

/// The window that decides a monitor: the first one down the z-order that
/// covers it or isn't always-on-top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecidingWindow {
    pub covers: bool,
    /// Maximized.
    pub zoomed: bool,
    /// One of Launchpad's own.
    pub own: bool,
    /// Belongs to the foreground app.
    pub foreground: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MonitorEntry {
    /// left, top, right, bottom
    pub monitor: [f64; 4],
    pub window: Option<DecidingWindow>,
}

/// The walk's result. `ok` false or no `monitors`: it failed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
    pub ok: bool,
    pub monitors: Option<Vec<MonitorEntry>>,
}

/// A window takes its monitor over only if it covers all of it, isn't
/// maximized (nothing reserves space, so a maximized window filling the
/// monitor is just a window), isn't one of Launchpad's own, and is in front:
/// it belongs to the foreground app, or Explorer says a fullscreen app is
/// running there (ABN_FULLSCREENAPP, which only docked panels hear). A
/// launcher, overlay or anti-cheat window a game leaves behind can cover a
/// monitor without either.
pub fn taken_over(win: Option<&DecidingWindow>, explorer_hint: bool) -> bool {
    match win {
        Some(w) if w.covers && !w.zoomed && !w.own => w.foreground || explorer_hint,
        _ => false,
    }
}

fn same_rect([l, t, r, b]: [f64; 4], p: Rect) -> bool {
    l == p.x && t == p.y && r == p.x + p.width && b == p.y + p.height
}

/// The new fullscreen state of a display whose bounds (physical) are `rect`,
/// given the reply and its previous state. A failed reply keeps the previous
/// state: it says nothing about the display, and assuming "not fullscreen"
/// would put the panel over a running game.
pub fn decide(reply: Option<&Reply>, rect: Rect, previous: bool, explorer_hint: bool) -> bool {
    let Some(monitors) = reply.filter(|r| r.ok).and_then(|r| r.monitors.as_ref()) else {
        return previous;
    };
    monitors
        .iter()
        .find(|m| same_rect(m.monitor, rect))
        .is_some_and(|entry| taken_over(entry.window.as_ref(), explorer_hint))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect::new(0.0, 0.0, 2560.0, 1440.0);
    const SECOND: Rect = Rect::new(2560.0, 0.0, 1920.0, 1080.0);
    const GAME: DecidingWindow = DecidingWindow { covers: true, zoomed: false, own: false, foreground: true };

    fn reply(entries: &[MonitorEntry]) -> Reply {
        Reply { ok: true, monitors: Some(entries.to_vec()) }
    }
    fn at(r: Rect, window: Option<DecidingWindow>) -> MonitorEntry {
        MonitorEntry { monitor: [r.x, r.y, r.x + r.width, r.y + r.height], window }
    }

    #[test]
    fn a_covering_window_of_the_foreground_app_takes_the_monitor_over() {
        assert!(taken_over(Some(&GAME), false));
    }

    #[test]
    fn a_covering_window_left_behind_the_foreground_a_launcher_an_overlay_does_not() {
        assert!(!taken_over(Some(&DecidingWindow { foreground: false, ..GAME }), false));
    }

    #[test]
    fn explorers_fullscreen_notice_counts_a_covering_window_that_isnt_in_front() {
        assert!(taken_over(Some(&DecidingWindow { foreground: false, ..GAME }), true));
    }

    #[test]
    fn maximized_own_partly_covering_or_no_window_never_takes_it_over() {
        let wins = [
            Some(DecidingWindow { zoomed: true, ..GAME }),
            Some(DecidingWindow { own: true, ..GAME }),
            Some(DecidingWindow { covers: false, ..GAME }),
            None,
        ];
        for win in wins {
            assert!(!taken_over(win.as_ref(), true));
        }
    }

    #[test]
    fn each_display_is_matched_to_its_own_monitor_in_the_reply() {
        let r = reply(&[at(PRIMARY, None), at(SECOND, Some(GAME))]);
        assert!(!decide(Some(&r), PRIMARY, true, false));
        assert!(decide(Some(&r), SECOND, false, false));
    }

    #[test]
    fn a_display_missing_from_the_reply_is_not_fullscreen() {
        assert!(!decide(Some(&reply(&[at(SECOND, Some(GAME))])), PRIMARY, true, false));
    }

    #[test]
    fn a_failed_reply_keeps_the_previous_state() {
        let failed = Reply { ok: false, monitors: None };
        let no_monitors = Reply { ok: true, monitors: None };
        for r in [Some(&failed), None, Some(&no_monitors)] {
            assert!(decide(r, PRIMARY, true, false));
            assert!(!decide(r, PRIMARY, false, false));
        }
    }
}
