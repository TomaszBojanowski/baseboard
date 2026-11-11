// SPDX-License-Identifier: GPL-3.0-or-later

//! The place of a card's status box (D60, D61): as high as the status box
//! of the card next to it in a row, whatever their numbers of lines; in
//! one column as high as its own box. The grid of cards pairs the slots.
//! `GtkSizeGroup` cannot do this with text that wraps: it compares its
//! widgets without the width they get, so a box that wraps to three lines
//! only in its column was compared by its two unwrapped lines. A slot
//! measures its own box and its peer's at the width it is given; the
//! columns of the cards are equally wide.

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct StatusSlot {
        pub(super) peer: glib::WeakRef<super::StatusSlot>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StatusSlot {
        const NAME: &'static str = "BaseboardStatusSlot";
        type Type = super::StatusSlot;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Generic);
        }
    }

    impl ObjectImpl for StatusSlot {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for StatusSlot {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (mut minimum, mut natural, _, _) = child.measure(orientation, for_size);
            if orientation == gtk::Orientation::Vertical
                && let Some(peer) = self.peer.upgrade().and_then(|p| p.first_child())
                && peer.is_visible()
            {
                let (peer_minimum, peer_natural, _, _) = peer.measure(orientation, for_size);
                minimum = minimum.max(peer_minimum);
                natural = natural.max(peer_natural);
            }
            (minimum, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub struct StatusSlot(ObjectSubclass<imp::StatusSlot>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StatusSlot {
    /// A slot holding `child`, which fills it.
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let slot: Self = glib::Object::new();
        child.set_parent(&slot);
        slot
    }

    /// Makes `a` and `b` equally high: they stand side by side.
    pub fn pair(a: &Self, b: &Self) {
        for (slot, peer) in [(a, b), (b, a)] {
            slot.imp().peer.set(Some(peer));
            slot.queue_resize();
        }
    }

    /// Leaves the slot alone, as in one column.
    pub fn unpair(&self) {
        if let Some(peer) = self.imp().peer.upgrade() {
            peer.imp().peer.set(None);
            peer.queue_resize();
        }
        self.imp().peer.set(None);
        self.queue_resize();
    }

    /// The content of the box changed: its peer measures again too. GTK
    /// lays out again only the widgets above a change, and the peer is not
    /// one of them.
    pub fn content_changed(&self) {
        self.queue_resize();
        if let Some(peer) = self.imp().peer.upgrade() {
            peer.queue_resize();
        }
    }
}
