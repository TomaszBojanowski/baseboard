// SPDX-License-Identifier: GPL-3.0-or-later

//! The layout of the cards on the Fans page as one rule (D61), decided by
//! width alone, never by the values shown at the moment:
//!
//! - the name and subtitle of a card never wrap;
//! - the header line has the speed and duty cycle on the right while the
//!   widest content fits next to the name: a four-digit speed, the duty
//!   cycle with a target ("duty 100 % → 100 %"), the longest name and
//!   subtitle of the page, in the language and font of the app;
//! - otherwise the speed and duty cycle go under the name, side by side,
//!   and stacked when even that does not fit;
//! - the cards stand in two columns while the header with the values under
//!   the name fits there, otherwise in one;
//! - every card of the page has the same layout, and the cards of a row
//!   are equally high.
//!
//! The needs come from the cards (`CardWidgets::needs`); the header lines
//! and the grid read them while GTK measures, so a new arrangement costs no
//! second pass. Only the pairing of the status boxes follows the columns
//! after the layout: GTK keeps the sizes it measured until a widget asks
//! for a new size, which cannot be asked for while laying out.

use std::cell::Cell;
use std::rc::Rc;

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;

use super::slot::StatusSlot;

/// Space between the icon and the texts, and between the texts and the
/// values on the right.
pub const GAP: i32 = 12;
/// Space between the name block and the values under it.
const ROW_GAP: i32 = 4;
/// Space between the cards of a row and between rows.
pub const SPACING: i32 = 18;
/// The horizontal padding of a card on each side (`.fan-card` in
/// style.css).
pub const CARD_PADDING: i32 = 18;

/// How the header line of a card is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// Speed and duty cycle on the right of the name.
    Wide,
    /// Speed and duty cycle side by side under the name.
    Below,
    /// Speed and duty cycle under the name, one under the other.
    Stacked,
}

/// The widths a header line needs in each arrangement for the widest
/// content, in pixels, without the card's padding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeadNeeds {
    pub wide: i32,
    pub below: i32,
    pub stacked: i32,
    /// The least width: the values stacked, the name shortened with an
    /// ellipsis. Only with very large text in the narrowest window.
    pub least: i32,
    /// The column of the values on the right: the widest speed or duty
    /// cycle.
    pub values: i32,
    /// The widest speed: under the name the duty cycle starts after it.
    pub rpm: i32,
}

impl HeadNeeds {
    /// From the widths of the icon, the name block (name or subtitle,
    /// whichever is wider), the widest speed and the widest duty cycle
    /// (0 without one).
    pub fn of(icon: i32, texts: i32, rpm: i32, duty: i32) -> Self {
        let start = icon + GAP;
        let values = rpm.max(duty);
        let row = if duty > 0 { rpm + GAP + duty } else { rpm };
        Self {
            wide: start + texts + GAP + values,
            below: start + texts.max(row),
            stacked: start + texts.max(values),
            least: start + values,
            values,
            rpm,
        }
    }

    /// The needs of a page: the most of each card.
    pub fn max(self, other: Self) -> Self {
        Self {
            wide: self.wide.max(other.wide),
            below: self.below.max(other.below),
            stacked: self.stacked.max(other.stacked),
            least: self.least.max(other.least),
            values: self.values.max(other.values),
            rpm: self.rpm.max(other.rpm),
        }
    }

    /// The arrangement of a header line `width` wide.
    pub fn arrangement(&self, width: i32) -> Arrangement {
        if width >= self.wide {
            Arrangement::Wide
        } else if width >= self.below {
            Arrangement::Below
        } else {
            Arrangement::Stacked
        }
    }

    /// The columns of cards in a grid `width` wide: two while the header
    /// with the values under the name fits in a card.
    pub fn columns(&self, width: i32) -> i32 {
        let column = (width - SPACING) / 2;
        if column - 2 * CARD_PADDING >= self.below {
            2
        } else {
            1
        }
    }
}

/// What the page tells its header lines and grid: the needs of its widest
/// content. The page sets it after updating its cards, outside layout, and
/// lays the header lines out again when it changes.
#[derive(Debug, Default)]
pub struct PageLayout {
    pub needs: Cell<HeadNeeds>,
}

// ---------- the header line of a card ----------

mod head_imp {
    use super::*;

    #[derive(Default)]
    pub struct CardHead {
        pub(super) layout: std::cell::OnceCell<Rc<PageLayout>>,
        pub(super) parts: std::cell::OnceCell<super::HeadParts>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CardHead {
        const NAME: &'static str = "BaseboardCardHead";
        type Type = super::CardHead;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Generic);
        }
    }

    impl ObjectImpl for CardHead {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for CardHead {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let needs = obj.needs();
            match orientation {
                gtk::Orientation::Horizontal => (needs.least, needs.wide, -1, -1),
                _ => {
                    let width = if for_size < 0 { needs.wide } else { for_size };
                    let height = obj.place(width, None);
                    (height, height, -1, -1)
                }
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().place(width, Some(height));
        }
    }
}

/// The parts of a header line.
#[derive(Debug)]
pub struct HeadParts {
    pub icon: gtk::Widget,
    /// The name and the subtitle, one above the other.
    pub texts: gtk::Widget,
    pub rpm: gtk::Widget,
    pub duty: gtk::Widget,
}

glib::wrapper! {
    pub struct CardHead(ObjectSubclass<head_imp::CardHead>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

fn natural(widget: &gtk::Widget, orientation: gtk::Orientation, for_size: i32) -> i32 {
    widget.measure(orientation, for_size).1
}

impl CardHead {
    pub fn new(parts: HeadParts, layout: Rc<PageLayout>) -> Self {
        let head: Self = glib::Object::new();
        for part in [&parts.icon, &parts.texts, &parts.rpm, &parts.duty] {
            part.set_parent(&head);
        }
        let _ = head.imp().parts.set(parts);
        let _ = head.imp().layout.set(layout);
        head
    }

    fn needs(&self) -> HeadNeeds {
        self.imp()
            .layout
            .get()
            .map(|layout| layout.needs.get())
            .unwrap_or_default()
    }

    /// The height of the header line `width` wide; with `height`, also
    /// places its parts.
    fn place(&self, width: i32, height: Option<i32>) -> i32 {
        let Some(parts) = self.imp().parts.get() else {
            return 0;
        };
        let needs = self.needs();
        let vertical = gtk::Orientation::Vertical;
        let icon_width = natural(&parts.icon, gtk::Orientation::Horizontal, -1);
        let icon_height = natural(&parts.icon, vertical, -1);
        let start = icon_width + GAP;
        let rpm_width = natural(&parts.rpm, gtk::Orientation::Horizontal, -1);
        let duty_width = natural(&parts.duty, gtk::Orientation::Horizontal, -1);
        let (_, rpm_height, _, rpm_base) = parts.rpm.measure(vertical, -1);
        let (_, duty_height, _, duty_base) = parts.duty.measure(vertical, -1);
        let put = |widget: &gtk::Widget, x: i32, y: i32, w: i32, h: i32| {
            if height.is_some() {
                widget.size_allocate(&gtk::Allocation::new(x, y, w.max(0), h), -1);
            }
        };

        match needs.arrangement(width) {
            Arrangement::Wide => {
                let values = needs.values.max(rpm_width).max(duty_width);
                let texts_width = (width - start - GAP - values).max(0);
                let texts_height = natural(&parts.texts, vertical, texts_width);
                let values_height = rpm_height + duty_height;
                let total = icon_height.max(texts_height).max(values_height);
                let h = height.unwrap_or(total);
                put(
                    &parts.icon,
                    0,
                    (h - icon_height) / 2,
                    icon_width,
                    icon_height,
                );
                put(
                    &parts.texts,
                    start,
                    (h - texts_height) / 2,
                    texts_width,
                    texts_height,
                );
                let y = (h - values_height) / 2;
                put(&parts.rpm, width - rpm_width, y, rpm_width, rpm_height);
                put(
                    &parts.duty,
                    width - duty_width,
                    y + rpm_height,
                    duty_width,
                    duty_height,
                );
                total
            }
            arrangement => {
                let texts_width = (width - start).max(0);
                let texts_height = natural(&parts.texts, vertical, texts_width);
                let top = icon_height.max(texts_height);
                put(
                    &parts.icon,
                    0,
                    (top - icon_height) / 2,
                    icon_width,
                    icon_height,
                );
                put(
                    &parts.texts,
                    start,
                    (top - texts_height) / 2,
                    texts_width,
                    texts_height,
                );
                let y = top + ROW_GAP;
                if arrangement == Arrangement::Below {
                    // Side by side on one baseline, as in the mockup.
                    let (above, row) = if rpm_base >= 0 && duty_base >= 0 {
                        let above = rpm_base.max(duty_base);
                        let below = (rpm_height - rpm_base).max(duty_height - duty_base);
                        (above, above + below)
                    } else {
                        (rpm_height.max(duty_height), rpm_height.max(duty_height))
                    };
                    let (rpm_y, duty_y) = if rpm_base >= 0 && duty_base >= 0 {
                        (y + above - rpm_base, y + above - duty_base)
                    } else {
                        (y + row - rpm_height, y + row - duty_height)
                    };
                    // The duty cycle after the widest speed, so that it
                    // does not move when the speed gets a digit.
                    let duty_x = start + needs.rpm.max(rpm_width) + GAP;
                    put(&parts.rpm, start, rpm_y, rpm_width, rpm_height);
                    put(&parts.duty, duty_x, duty_y, duty_width, duty_height);
                    y + row
                } else {
                    put(&parts.rpm, start, y, rpm_width, rpm_height);
                    put(&parts.duty, start, y + rpm_height, duty_width, duty_height);
                    y + rpm_height + duty_height
                }
            }
        }
    }
}

// ---------- the grid of cards ----------

mod grid_imp {
    use super::*;

    #[derive(Default)]
    pub struct CardsGrid {
        pub(super) layout: std::cell::OnceCell<Rc<PageLayout>>,
        /// The columns at the last allocation; 0 before the first.
        pub(super) columns: Cell<i32>,
        /// The status boxes are paired for this many columns.
        pub(super) paired: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CardsGrid {
        const NAME: &'static str = "BaseboardCardsGrid";
        type Type = super::CardsGrid;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Generic);
        }
    }

    impl ObjectImpl for CardsGrid {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for CardsGrid {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let cards = obj.cards();
            match orientation {
                gtk::Orientation::Horizontal => {
                    let (mut minimum, mut natural) = (0, 0);
                    for card in &cards {
                        let (min, nat, _, _) = card.measure(orientation, -1);
                        minimum = minimum.max(min);
                        natural = natural.max(nat);
                    }
                    if cards.len() > 1 {
                        natural = 2 * natural + SPACING;
                    }
                    (minimum, natural, -1, -1)
                }
                _ => {
                    let width = if for_size < 0 {
                        obj.measure(gtk::Orientation::Horizontal, -1).1
                    } else {
                        for_size
                    };
                    // A scrolled page gives its child the minimum height:
                    // the whole grid.
                    let height = obj.place(width, false);
                    (height, height, -1, -1)
                }
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let obj = self.obj();
            let columns = obj.place(width, true);
            if self.columns.replace(columns) != columns && self.paired.get() != columns {
                // The status boxes measure their peers, so they are paired
                // anew for the other number of columns; not during layout,
                // where GTK keeps the sizes it measured.
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    obj,
                    move || obj.pair_slots()
                ));
            }
        }
    }
}

glib::wrapper! {
    pub struct CardsGrid(ObjectSubclass<grid_imp::CardsGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CardsGrid {
    pub fn new(layout: Rc<PageLayout>) -> Self {
        let grid: Self = glib::Object::new();
        let _ = grid.imp().layout.set(layout);
        grid
    }

    /// Puts `cards` in the grid in place of those it had, and pairs their
    /// status boxes by rows.
    pub fn set_cards(&self, cards: &[&gtk::Widget]) {
        for slot in self.slots().into_iter().flatten() {
            slot.unpair();
        }
        while let Some(child) = self.first_child() {
            child.unparent();
        }
        for card in cards {
            card.set_parent(self);
        }
        self.pair_slots();
    }

    /// The status box of each card shown, if it has one.
    fn slots(&self) -> Vec<Option<StatusSlot>> {
        self.cards()
            .iter()
            .map(|card| {
                let mut child = card.first_child();
                while let Some(widget) = child {
                    if let Ok(slot) = widget.clone().downcast::<StatusSlot>() {
                        return Some(slot);
                    }
                    child = widget.next_sibling();
                }
                None
            })
            .collect()
    }

    /// In two columns the status boxes of a row are equally high, so that
    /// the charts and the footers line up (D60); in one column each box is
    /// as high as its text. Before the first allocation the grid pairs for
    /// two columns, the usual case.
    pub fn pair_slots(&self) {
        let imp = self.imp();
        let columns = match imp.columns.get() {
            0 => 2,
            columns => columns,
        };
        let slots = self.slots();
        for slot in slots.iter().flatten() {
            slot.unpair();
        }
        if columns == 2 {
            for row in slots.chunks(2) {
                if let [Some(left), Some(right)] = row {
                    StatusSlot::pair(left, right);
                }
            }
        }
        imp.paired.set(columns);
    }

    fn cards(&self) -> Vec<gtk::Widget> {
        let mut cards = Vec::new();
        let mut child = self.first_child();
        while let Some(card) = child {
            child = card.next_sibling();
            if card.is_visible() {
                cards.push(card);
            }
        }
        cards
    }

    /// The columns of the grid `width` wide: two while the header line of a
    /// card fits with its values under the name and every card fits.
    fn columns(&self, width: i32, cards: &[gtk::Widget]) -> i32 {
        let Some(layout) = self.imp().layout.get() else {
            return 1;
        };
        let least = cards
            .iter()
            .map(|card| card.measure(gtk::Orientation::Horizontal, -1).0)
            .max()
            .unwrap_or(0);
        if layout.needs.get().columns(width) == 2 && (width - SPACING) / 2 >= least {
            2
        } else {
            1
        }
    }

    /// The height of the grid `width` wide, or with `allocate` its columns,
    /// after placing the cards in rows as high as their higher card.
    fn place(&self, width: i32, allocate: bool) -> i32 {
        let cards = self.cards();
        let columns = self.columns(width, &cards);
        let column = if columns == 2 {
            (width - SPACING) / 2
        } else {
            width
        };
        let mut y = 0;
        for (i, row) in cards.chunks(columns as usize).enumerate() {
            if i > 0 {
                y += SPACING;
            }
            let height = row
                .iter()
                .map(|card| card.measure(gtk::Orientation::Vertical, column).1)
                .max()
                .unwrap_or(0);
            if allocate {
                for (j, card) in row.iter().enumerate() {
                    let x = j as i32 * (column + SPACING);
                    card.size_allocate(&gtk::Allocation::new(x, y, column, height), -1);
                }
            }
            y += height;
        }
        if allocate { columns } else { y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Icon 40, name 190, speed 110, duty with a target 170: the widths of
    /// each arrangement.
    fn needs() -> HeadNeeds {
        HeadNeeds::of(40, 190, 110, 170)
    }

    #[test]
    fn widths_of_the_arrangements() {
        let needs = needs();
        assert_eq!(needs.wide, 40 + 12 + 190 + 12 + 170);
        assert_eq!(needs.below, 40 + 12 + (110 + 12 + 170));
        assert_eq!(needs.stacked, 40 + 12 + 190);
        assert_eq!(needs.values, 170);
        // Without a duty cycle (a driver without control), the speed alone.
        let low = HeadNeeds::of(40, 190, 110, 0);
        assert_eq!(low.wide, 40 + 12 + 190 + 12 + 110);
        assert_eq!(low.below, 40 + 12 + 190);
    }

    /// The arrangement follows the width only.
    #[test]
    fn arrangement_by_width() {
        let needs = needs();
        assert_eq!(needs.arrangement(needs.wide), Arrangement::Wide);
        assert_eq!(needs.arrangement(needs.wide - 1), Arrangement::Below);
        assert_eq!(needs.arrangement(needs.below), Arrangement::Below);
        assert_eq!(needs.arrangement(needs.below - 1), Arrangement::Stacked);
    }

    /// Two columns while a card can show the values under the name.
    #[test]
    fn columns_by_width() {
        let needs = needs();
        let two = 2 * (needs.below + 2 * CARD_PADDING) + SPACING;
        assert_eq!(needs.columns(two), 2);
        assert_eq!(needs.columns(two - 2), 1);
        // The page's needs are the most of its cards.
        let wider = HeadNeeds::of(40, 260, 110, 170);
        assert_eq!(needs.max(wider).below, wider.below);
        assert_eq!(needs.max(wider).wide, wider.wide);
    }
}
