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
