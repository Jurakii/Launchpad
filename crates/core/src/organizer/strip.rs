//! Geometry for the layout strip that drops down from the top of a display
//! while a window is being dragged. The caller hit-tests the cursor against
//! these rects and the overlay draws exactly the same rects, so what the user
//! sees highlighted is what they'll get. Everything is in DIP screen
//! coordinates.
use super::zones::{zone_rect, Layout};
use super::{js_round, Point, Rect};

const PAD: f64 = 14.0; // strip edge to thumbnails
const THUMB_GAP: f64 = 14.0; // between thumbnails
const MAX_THUMB_W: f64 = 96.0;
const MIN_THUMB_W: f64 = 40.0;
const LABEL_H: f64 = 20.0; // layout name under each thumbnail

// How close to the top of the work area the cursor has to get before the
// strip appears, and how far outside the strip it can wander before the
// strip goes away again. The margin stops it flickering at the edges.
pub const TRIGGER_DIP: f64 = 48.0;
pub const STAY_MARGIN_DIP: f64 = 48.0;

#[derive(Debug, Clone, PartialEq)]
pub struct StripZone {
    pub zone_id: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StripItem {
    pub layout_id: String,
    pub name: String,
    pub thumb: Rect,
    pub zones: Vec<StripZone>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Strip {
    pub rect: Rect,
    pub items: Vec<StripItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoneHit {
    pub layout_id: String,
    pub zone_id: String,
}

/// One thumbnail per layout, centred along the top of the work area.
/// Thumbnails share the display's aspect ratio so zones look like the result,
/// and shrink when many layouts have to fit on a narrow display.
pub fn compute_strip(layouts: &[Layout], work_area: Rect) -> Strip {
    let n = layouts.len() as f64;
    let fit_w = ((work_area.width - 2.0 * PAD - (n - 1.0) * THUMB_GAP) / n).floor();
    let thumb_w = MIN_THUMB_W.max(MAX_THUMB_W.min(fit_w));
    let thumb_h = js_round(thumb_w * work_area.height / work_area.width);
    let width = 2.0 * PAD + n * thumb_w + (n - 1.0) * THUMB_GAP;
    let height = PAD + thumb_h + LABEL_H + PAD / 2.0;
    let x = work_area.x + js_round((work_area.width - width) / 2.0);
    let y = work_area.y;

    let items = layouts
        .iter()
        .enumerate()
        .map(|(i, layout)| {
            let thumb = Rect { x: x + PAD + i as f64 * (thumb_w + THUMB_GAP), y: y + PAD, width: thumb_w, height: thumb_h };
            StripItem {
                layout_id: layout.id.clone(),
                name: layout.name.clone(),
                thumb,
                // No gap: the zones tile the whole thumbnail, so the cursor is always
                // over some zone while it's over a thumbnail (the overlay draws them
                // slightly inset to show the divisions).
                zones: layout
                    .zones
                    .iter()
                    .map(|z| StripZone { zone_id: z.id.clone(), rect: zone_rect(layout, z, thumb, 0) })
                    .collect(),
            }
        })
        .collect();
    Strip { rect: Rect { x, y, width, height }, items }
}

/// The layout and zone under point p, if any.
pub fn zone_at(strip: &Strip, p: Point) -> Option<ZoneHit> {
    let item = strip.items.iter().find(|item| item.thumb.contains(p))?;
    let zone = item.zones.iter().find(|z| z.rect.contains(p))?;
    Some(ZoneHit { layout_id: item.layout_id.clone(), zone_id: zone.zone_id.clone() })
}

/// Whether the strip should be showing for cursor p. It appears once the
/// cursor nears the top of the work area (including above it, e.g. over a
/// Windows taskbar docked at the top), and stays while the cursor is over or
/// near the strip so the user can move down to the thumbnails.
pub fn should_show(strip: &Strip, work_area: Rect, p: Point, was_showing: bool) -> bool {
    if p.y < work_area.y + TRIGGER_DIP {
        return true;
    }
    // Being where the strip is drawn counts too: a quick flick to the top and
    // back down onto a thumbnail can pass through the band between two polls.
    if strip.rect.contains(p) {
        return true;
    }
    if !was_showing {
        return false;
    }
    let r = strip.rect;
    let stay = Rect {
        x: r.x - STAY_MARGIN_DIP,
        y: r.y,
        width: r.width + 2.0 * STAY_MARGIN_DIP,
        height: r.height + STAY_MARGIN_DIP,
    };
    stay.contains(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organizer::layouts::built_in_layouts;

    const WA: Rect = Rect::new(0.0, 32.0, 2560.0, 1408.0); // primary under a 32px top bar
    const SECOND: Rect = Rect::new(-1920.0, 0.0, 1920.0, 1080.0); // left of the primary

    fn right(r: Rect) -> f64 {
        r.x + r.width
    }
    fn bottom(r: Rect) -> f64 {
        r.y + r.height
    }
    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x && inner.y >= outer.y && right(inner) <= right(outer) && bottom(inner) <= bottom(outer)
    }
    fn centre(r: Rect) -> Point {
        Point { x: r.x + r.width / 2.0, y: r.y + r.height / 2.0 }
    }
    fn hit(layout_id: &str, zone_id: &str) -> Option<ZoneHit> {
        Some(ZoneHit { layout_id: layout_id.into(), zone_id: zone_id.into() })
    }
    fn item<'a>(s: &'a Strip, layout_id: &str) -> &'a StripItem {
        s.items.iter().find(|i| i.layout_id == layout_id).unwrap()
    }

    #[test]
    fn compute_strip_one_thumbnail_per_layout_centred_along_the_top_of_the_work_area() {
        let layouts = built_in_layouts();
        for wa in [WA, SECOND] {
            let s = compute_strip(&layouts, wa);
            assert_eq!(
                s.items.iter().map(|i| i.layout_id.as_str()).collect::<Vec<_>>(),
                layouts.iter().map(|l| l.id.as_str()).collect::<Vec<_>>()
            );
            assert_eq!(s.rect.y, wa.y);
            assert!((centre(s.rect).x - centre(wa).x).abs() <= 1.0, "centred");
            assert!(inside(s.rect, wa), "inside the work area");
            for item in &s.items {
                assert!(inside(item.thumb, s.rect), "{} thumbnail inside the strip", item.layout_id);
                for z in &item.zones {
                    assert!(inside(z.rect, item.thumb), "{}/{} inside its thumbnail", item.layout_id, z.zone_id);
                }
            }
            for i in 1..s.items.len() {
                assert!(
                    s.items[i].thumb.x >= right(s.items[i - 1].thumb),
                    "thumbnails left to right without overlapping"
                );
            }
        }
    }

    #[test]
    fn compute_strip_thumbnails_share_the_display_shape_and_shrink_to_fit_a_narrow_display() {
        let layouts = built_in_layouts();
        let wide = compute_strip(&layouts, WA).items[0].thumb;
        assert_eq!(wide.width, 96.0);
        assert_eq!(wide.height, js_round(96.0 * WA.height / WA.width));

        let narrow = Rect::new(0.0, 0.0, 400.0, 800.0);
        let s = compute_strip(&layouts, narrow);
        assert!(s.items[0].thumb.width < 96.0);
        assert!(s.rect.width <= narrow.width);
    }

    #[test]
    fn zone_at_finds_the_zone_under_the_cursor_and_nothing_between_or_outside_thumbnails() {
        let s = compute_strip(&built_in_layouts(), WA);
        for z in &item(&s, "quarters").zones {
            assert_eq!(zone_at(&s, centre(z.rect)), hit("quarters", &z.zone_id));
        }
        // Every point of a thumbnail belongs to some zone, including the edges.
        let halves = item(&s, "halves").thumb;
        assert_eq!(zone_at(&s, Point { x: halves.x, y: halves.y }), hit("halves", "left"));
        assert_eq!(zone_at(&s, Point { x: right(halves) - 1.0, y: bottom(halves) - 1.0 }), hit("halves", "right"));

        let between_thumbs = Point { x: right(s.items[0].thumb) + 2.0, y: centre(s.items[0].thumb).y };
        assert_eq!(zone_at(&s, between_thumbs), None);
        assert_eq!(zone_at(&s, centre(WA)), None);
    }

    #[test]
    fn should_show_appears_near_the_top_stays_while_near_the_strip_hides_once_the_cursor_leaves() {
        let s = compute_strip(&built_in_layouts(), WA);
        let mid_x = centre(s.rect).x;
        let below_trigger = WA.y + TRIGGER_DIP + 5.0;
        let at = |x: f64, y: f64| Point { x, y };

        assert!(should_show(&s, WA, at(mid_x, WA.y + 10.0), false), "near the top");
        assert!(should_show(&s, WA, at(200.0, WA.y + 10.0), false), "anywhere along the top");
        assert!(should_show(&s, WA, at(mid_x, WA.y - 20.0), false), "above the work area (top taskbar)");
        assert!(!should_show(&s, WA, at(200.0, below_trigger), false), "not before reaching the top");
        assert!(!should_show(&s, WA, at(mid_x, bottom(s.rect) + 5.0), false), "not just below the strip");
        // A flick can skip the band entirely between polls; landing on the strip still shows it.
        let thumb = s.items[2].thumb;
        let low_on_thumb = at(centre(thumb).x, bottom(thumb) - 2.0);
        assert!(low_on_thumb.y >= WA.y + TRIGGER_DIP, "the test needs a point below the trigger band");
        assert!(should_show(&s, WA, low_on_thumb, false), "over the strip area");

        // Once showing, it stays within a margin around the strip.
        assert!(should_show(&s, WA, low_on_thumb, true));
        assert!(should_show(&s, WA, at(mid_x, bottom(s.rect) + STAY_MARGIN_DIP - 1.0), true), "within the margin");
        assert!(!should_show(&s, WA, at(mid_x, bottom(s.rect) + STAY_MARGIN_DIP + 1.0), true), "past the margin");
        assert!(!should_show(&s, WA, at(10.0, below_trigger), true), "off to the side");
    }

    // Not in the JS tests: the strip's x is the one place a negative value is rounded.
    #[test]
    fn a_strip_wider_than_the_work_area_is_centred_with_math_round() {
        // 5 thumbnails at the 40 minimum need 284; on 199 that is -42.5 a side.
        let s = compute_strip(&built_in_layouts(), Rect::new(0.0, 0.0, 199.0, 400.0));
        assert_eq!((s.rect.width, s.rect.x), (284.0, -42.0));
    }
}
