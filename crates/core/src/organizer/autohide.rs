//! When an auto-hiding organizer panel comes down and goes away again, decided
//! from cursor samples: nothing of Launchpad's gets mouse events while the
//! cursor is over other apps, so the caller polls the cursor and feeds it
//! here. Everything is in DIP.
use super::{Point, Rect};

// The cursor has to rest against the top edge this long before the panel
// comes down. Without the wait, flicking up to a maximized window's title
// bar or close button would put the panel in the way of the click.
pub const REVEAL_DELAY_MS: i64 = 250;
// How far below or beside the panel the cursor can go, and for how long,
// before the panel goes away again.
pub const STAY_MARGIN_DIP: f64 = 16.0;
pub const HIDE_DELAY_MS: i64 = 500;
// The band along the very top of the display that counts as the edge.
pub const EDGE_DIP: f64 = 2.0;

/// `since` is when the cursor started doing what will change the panel's
/// state (resting at the edge, or being away).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    pub revealed: bool,
    pub since: Option<i64>,
}

/// One cursor sample. `bounds`: the display's bounds. `panel`: the panel's
/// rect. `can_reveal` false (a window is being dragged, or a fullscreen app
/// has the display) keeps a hidden panel hidden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub cursor: Point,
    pub bounds: Rect,
    pub panel: Rect,
    pub now: i64,
    pub can_reveal: bool,
}

pub fn initial(revealed: bool) -> State {
    State { revealed, since: None }
}

/// `state` is what the last call returned.
pub fn step(state: State, sample: Sample) -> State {
    let Sample { cursor: p, bounds, panel, now, can_reveal } = sample;
    if !state.revealed {
        let edge = Rect { x: bounds.x, y: bounds.y, width: bounds.width, height: EDGE_DIP };
        if !can_reveal || !edge.contains(p) {
            return initial(false);
        }
        let since = state.since.unwrap_or(now);
        return if now - since >= REVEAL_DELAY_MS { initial(true) } else { State { revealed: false, since: Some(since) } };
    }
    let stay = Rect {
        x: panel.x - STAY_MARGIN_DIP,
        y: panel.y,
        width: panel.width + 2.0 * STAY_MARGIN_DIP,
        height: panel.height + STAY_MARGIN_DIP,
    };
    if stay.contains(p) {
        return initial(true);
    }
    let since = state.since.unwrap_or(now);
    if now - since >= HIDE_DELAY_MS {
        initial(false)
    } else {
        State { revealed: true, since: Some(since) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rect = Rect::new(-1920.0, 0.0, 1920.0, 1080.0); // a display left of the primary
    const PANEL: Rect = Rect::new(-1920.0, 0.0, 1920.0, 32.0);
    const EDGE: Point = Point { x: -1000.0, y: 0.0 };
    const BELOW: Point = Point { x: -1000.0, y: 400.0 };

    // Feeds cursor samples (ms, point) in order and returns the state after each.
    fn run_from(samples: &[(i64, Point)], can_reveal: bool, from: State) -> Vec<bool> {
        let mut state = from;
        samples
            .iter()
            .map(|&(now, cursor)| {
                state = step(state, Sample { cursor, bounds: BOUNDS, panel: PANEL, now, can_reveal });
                state.revealed
            })
            .collect()
    }
    fn run(samples: &[(i64, Point)]) -> Vec<bool> {
        run_from(samples, true, initial(false))
    }

    #[test]
    fn resting_against_the_top_edge_reveals_the_panel_but_only_after_the_delay() {
        let d = REVEAL_DELAY_MS;
        assert_eq!(run(&[(0, EDGE), (d - 50, EDGE), (d, EDGE)]), [false, false, true]);
    }

    #[test]
    fn passing_through_the_top_edge_on_the_way_to_a_click_does_not_reveal_it() {
        let d = REVEAL_DELAY_MS;
        assert_eq!(run(&[(0, EDGE), (100, BELOW), (d, EDGE), (d + 100, EDGE)]), [false, false, false, false]);
    }

    #[test]
    fn the_edge_of_another_display_or_the_edge_while_blocked_never_reveals_it() {
        let d = REVEAL_DELAY_MS;
        let primary_top = Point { x: 500.0, y: 0.0 };
        assert_eq!(run(&[(0, primary_top), (d * 2, primary_top)]), [false, false]);
        assert_eq!(run_from(&[(0, EDGE), (d * 2, EDGE)], false, initial(false)), [false, false]);
    }

    #[test]
    fn once_shown_it_stays_while_the_cursor_is_on_or_just_below_it_and_goes_after_the_delay_away() {
        let h = HIDE_DELAY_MS;
        let just_below = Point { x: -1000.0, y: PANEL.height + STAY_MARGIN_DIP - 1.0 };
        let from = initial(true);
        assert_eq!(run_from(&[(0, Point { x: -1000.0, y: 10.0 }), (1000, just_below)], true, from), [true, true]);
        assert_eq!(run_from(&[(0, BELOW), (h - 50, BELOW), (h, BELOW)], true, from), [true, true, false]);
        // Coming back before the delay is up keeps it, and restarts the clock.
        assert_eq!(
            run_from(&[(0, BELOW), (h - 50, EDGE), (h + 100, BELOW), (2 * h, BELOW)], true, from),
            [true, true, true, true]
        );
    }
}
