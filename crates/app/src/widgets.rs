// SPDX-License-Identifier: GPL-3.0-or-later

//! Small widgets that several pages build the same way.

/// The arrow at the end of a row or a card that opens another page or
/// view: dimmed, and for the eyes only, since the row or card says where it
/// leads.
pub fn chevron() -> gtk::Image {
    gtk::Image::builder()
        .icon_name("go-next-symbolic")
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .css_classes(["dimmed"])
        .build()
}
