// SPDX-License-Identifier: GPL-3.0-or-later

//! Small widgets that several pages build the same way.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

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

/// Shows the icon `name` in `image`, unless it shows it already. GTK clears
/// an image on every call, also with the same name, and looks its icon up
/// again in the icon cache, where a race of GTK 4.24 can end the app (GTK
/// issue 8286); readings shown once a second would do that every second.
pub fn set_icon(image: &gtk::Image, name: &str) {
    if image.icon_name().as_deref() != Some(name) {
        image.set_icon_name(Some(name));
    }
}

/// Gives `widget` the style classes `classes`, unless it has just these,
/// in any order. GTK takes every class off and puts them back on every
/// call, so that the style is worked out again also when nothing changes.
pub fn set_classes(widget: &impl IsA<gtk::Widget>, classes: &[&str]) {
    let current = widget.css_classes();
    let same = current.len() == classes.len()
        && classes
            .iter()
            .all(|class| current.iter().any(|now| now == class));
    if !same {
        widget.set_css_classes(classes);
    }
}

/// Gives `row` the description a screen reader says after its title, and
/// keeps it up to date: the value on its right, if it has one, and its
/// subtitle. libadwaita describes a row by a relation to its subtitle that
/// reaches screen readers empty, and GTK prefers that relation to a
/// description, so the relation goes; without this a row reached with Tab
/// is read by its title alone.
pub fn describe_row(row: &adw::ActionRow, value: Option<&gtk::Label>) {
    let update: Rc<dyn Fn()> = {
        let row = row.downgrade();
        let value = value.map(ObjectExt::downgrade);
        Rc::new(move || {
            if let Some(row) = row.upgrade() {
                let value = value.as_ref().and_then(glib::WeakRef::upgrade);
                let text = row_description(&row, value.as_ref());
                let widget = row.upcast_ref::<gtk::Widget>();
                widget.reset_relation(gtk::AccessibleRelation::DescribedBy);
                widget.update_property(&[gtk::accessible::Property::Description(&text)]);
            }
        })
    };
    update();
    let on_subtitle = update.clone();
    row.connect_subtitle_notify(move |_| on_subtitle());
    if let Some(value) = value {
        value.connect_label_notify(move |_| update());
    }
}

/// The value of `row` and its subtitle, each where there is one.
fn row_description(row: &adw::ActionRow, value: Option<&gtk::Label>) -> String {
    let value = value.map(|value| value.label().to_string());
    let subtitle = row.subtitle().map(|subtitle| subtitle.to_string());
    value
        .into_iter()
        .chain(subtitle)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(". ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gtk_test::notifications;

    /// The same icon and the same classes, in any order, leave the image as
    /// it is; another icon or another class changes it. Needs a display.
    #[test]
    fn icons_and_classes_only_change_when_they_differ() {
        crate::gtk_test::run("icons_and_classes_only_change_when_they_differ", || {
            let image = gtk::Image::from_icon_name("go-next-symbolic");
            image.set_css_classes(&["status-icon", "success"]);
            let icon = |name: &str| notifications(&image, "icon-name", || set_icon(&image, name));
            assert_eq!(icon("go-next-symbolic"), 0);
            assert_eq!(icon("dialog-warning-symbolic"), 1);
            assert_eq!(
                image.icon_name().as_deref(),
                Some("dialog-warning-symbolic")
            );
            let classes =
                |list: &[&str]| notifications(&image, "css-classes", || set_classes(&image, list));
            assert_eq!(classes(&["status-icon", "success"]), 0);
            assert_eq!(classes(&["success", "status-icon"]), 0);
            assert_eq!(classes(&["status-icon", "warning"]), 1);
            assert!(image.has_css_class("warning") && !image.has_css_class("success"));
            assert_eq!(classes(&["status-icon"]), 1);
        });
    }

    /// A screen reader hears the value of a row and its subtitle after its
    /// title, each where there is one. Needs a display.
    #[test]
    fn description_of_a_row() {
        crate::gtk_test::run("description_of_a_row", || {
            let row = adw::ActionRow::builder().title("Minimum").build();
            let value = gtk::Label::new(Some("30\u{a0}%"));
            assert_eq!(row_description(&row, None), "");
            assert_eq!(row_description(&row, Some(&value)), "30\u{a0}%");
            row.set_subtitle("The lowest duty cycle Baseboard allows");
            assert_eq!(
                row_description(&row, Some(&value)),
                "30\u{a0}%. The lowest duty cycle Baseboard allows"
            );
            value.set_label("");
            assert_eq!(
                row_description(&row, Some(&value)),
                "The lowest duty cycle Baseboard allows"
            );
        });
    }
}
