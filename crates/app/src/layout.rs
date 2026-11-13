// SPDX-License-Identifier: GPL-3.0-or-later

//! The widths of every page, in one place (docs/PROJEKT.md §5, D67). The
//! content grows with the window up to [`MAX_WIDTH`]; below that it gets
//! denser in steps, and in a narrow window it is one column. A page puts
//! its content in an `AdwBreakpointBin` and lets [`watch`] tell it the
//! width; the breakpoints apply outside of the layout, so a page may
//! rebuild its rows then.

use adw::prelude::*;

/// The widest the content of a page gets, in pixels at the default text
/// size.
pub const MAX_WIDTH: i32 = 1320;

/// How wide the content of a page is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Width {
    /// Under 600 sp: one column (a window of a phone, or half a screen).
    Narrow,
    /// 600–879 sp: one card a row, its parts side by side (a window of
    /// about 1024 px with the sidebar).
    Medium,
    /// 880–1099 sp: two or three columns, smaller (about 1200 px).
    Compact,
    /// 1100 sp and more.
    #[default]
    Wide,
}

impl Width {
    /// More than one column fits.
    pub fn columns(self) -> bool {
        matches!(self, Self::Compact | Self::Wide)
    }
}

/// Up to this width, in sp, a window or a page is narrow.
const NARROW: f64 = 599.0;

/// The breakpoints, widest first: a bin uses the last one that applies.
const BREAKPOINTS: [(Width, f64); 3] = [
    (Width::Compact, 1099.0),
    (Width::Medium, 879.0),
    (Width::Narrow, NARROW),
];

fn up_to(max: f64) -> adw::Breakpoint {
    adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        max,
        adw::LengthUnit::Sp,
    ))
}

/// The breakpoint of a narrow window, for the window to add its setters
/// to: it is narrow when its pages are.
pub fn narrow_window() -> adw::Breakpoint {
    up_to(NARROW)
}

/// Lets `clamp` grow with the window up to [`MAX_WIDTH`].
pub fn limit(clamp: &adw::Clamp) {
    clamp.set_maximum_size(MAX_WIDTH);
    clamp.set_tightening_threshold(MAX_WIDTH);
}

/// Adds the breakpoints to `bin` and calls `changed` with the width each
/// time it changes, and once now.
pub fn watch(bin: &adw::BreakpointBin, changed: impl Fn(Width) + 'static) {
    let mut breakpoints = Vec::new();
    for (width, max) in BREAKPOINTS {
        let breakpoint = up_to(max);
        bin.add_breakpoint(breakpoint.clone());
        breakpoints.push((breakpoint, width));
    }
    let width_of = move |bin: &adw::BreakpointBin| {
        let current = bin.current_breakpoint();
        breakpoints
            .iter()
            .find(|(breakpoint, _)| Some(breakpoint) == current.as_ref())
            .map_or(Width::Wide, |&(_, width)| width)
    };
    changed(width_of(bin));
    bin.connect_current_breakpoint_notify(move |bin| changed(width_of(bin)));
}

/// Where a group of [`Columns`] goes when there are two columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Left,
    Right,
    /// Into the column with fewer rows so far: the group has this many.
    Balanced(usize),
}

/// Groups of a page in one column, in the order they came, or side by
/// side in two when the page is wide enough (D67).
pub struct Columns {
    pub widget: gtk::Box,
    left: gtk::Box,
    right: gtk::Box,
    groups: std::cell::RefCell<Vec<(gtk::Widget, Place)>>,
    two: std::cell::Cell<bool>,
}

impl Columns {
    pub fn new() -> Self {
        let column = || {
            gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(24)
                .valign(gtk::Align::Start)
                .hexpand(true)
                .build()
        };
        let (left, right) = (column(), column());
        let widget = gtk::Box::builder().spacing(24).homogeneous(true).build();
        widget.append(&left);
        widget.append(&right);
        right.set_visible(false);
        Self {
            widget,
            left,
            right,
            groups: std::cell::RefCell::new(Vec::new()),
            two: std::cell::Cell::new(false),
        }
    }

    /// Takes every group off.
    pub fn clear(&self) {
        for (group, _) in self.groups.borrow_mut().drain(..) {
            if let Some(column) = group.parent().and_downcast::<gtk::Box>() {
                column.remove(&group);
            }
        }
    }

    pub fn push(&self, group: &impl IsA<gtk::Widget>, place: Place) {
        self.groups
            .borrow_mut()
            .push((group.clone().upcast(), place));
        self.arrange();
    }

    /// Two columns or one.
    pub fn set_two(&self, two: bool) {
        if self.two.replace(two) != two {
            self.arrange();
        }
    }

    fn arrange(&self) {
        let groups = self.groups.borrow();
        for (group, _) in groups.iter() {
            if let Some(column) = group.parent().and_downcast::<gtk::Box>() {
                column.remove(group);
            }
        }
        let two = self.two.get();
        let (mut left_rows, mut right_rows) = (0, 0);
        for (group, place) in groups.iter() {
            let right = two
                && match *place {
                    Place::Left => false,
                    Place::Right => true,
                    Place::Balanced(rows) => {
                        let right = right_rows < left_rows;
                        if right {
                            right_rows += rows;
                        } else {
                            left_rows += rows;
                        }
                        right
                    }
                };
            if right {
                self.right.append(group);
            } else {
                self.left.append(group);
            }
        }
        self.right
            .set_visible(two && self.right.first_child().is_some());
    }
}

impl Default for Columns {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two columns: groups pinned to their side, the others to the column
    /// with fewer rows; one column: the order they came in. Needs a
    /// display.
    #[test]
    fn groups_in_columns() {
        crate::gtk_test::run("groups_in_columns", || {
            let columns = Columns::new();
            let label = |text: &str| gtk::Label::new(Some(text));
            let (a, b, c, d) = (label("a"), label("b"), label("c"), label("d"));
            columns.push(&a, Place::Balanced(5));
            columns.push(&b, Place::Balanced(2));
            columns.push(&c, Place::Balanced(2));
            columns.push(&d, Place::Left);
            let side = |w: &gtk::Label| w.parent() == Some(columns.right.clone().upcast());
            assert!(!side(&a) && !side(&b) && !columns.right.is_visible());
            columns.set_two(true);
            assert!(!side(&a) && side(&b) && side(&c) && !side(&d));
            assert!(columns.right.get_visible());
            columns.clear();
            assert!(columns.left.first_child().is_none());
        });
    }

    /// The widths of the windows the owner uses, with the sidebar of a
    /// quarter of the window (180–280 px): 1850 and 1440 px wide, 1200 px
    /// compact, 1024 px one card a row; a window under 600 px has no
    /// sidebar beside the page.
    #[test]
    fn widths_of_the_windows() {
        let content = |window: f64| {
            let sidebar = if window < 600.0 {
                0.0
            } else {
                (window * 0.25).clamp(180.0, 280.0)
            };
            let width = window - sidebar;
            BREAKPOINTS
                .iter()
                .rev()
                .find(|&&(_, max)| width <= max)
                .map_or(Width::Wide, |&(w, _)| w)
        };
        assert_eq!(content(1850.0), Width::Wide);
        assert_eq!(content(1440.0), Width::Wide);
        assert_eq!(content(1200.0), Width::Compact);
        assert_eq!(content(1024.0), Width::Medium);
        assert_eq!(content(370.0), Width::Narrow);
        assert!(Width::Compact.columns() && !Width::Medium.columns());
    }
}
