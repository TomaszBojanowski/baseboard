// SPDX-License-Identifier: GPL-3.0-or-later

//! The curve details of one header, a subpage of the Fans page
//! (docs/PROJEKT.md §7 M2, M2.4, D62, makiety `wentylatory-szczegoly.html`):
//! the large chart with the legend of its marks, and a legend for what the
//! chart could not label, or that the curve could not be read; the points
//! as read from the board's controller; what the list of fans does not show
//! about the header (its mode, minimum, and whether it was checked and
//! spins); the temperature source and how a curve works.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use baseboard_core::fans::curve;
use baseboard_core::fans::driver::{CurvePoint, Mode, Source};
use baseboard_core::fans::page::{Chart, Fan, SourceRow};
use baseboard_core::overview::Subtitle;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{Numbers, UNIT_SPACE, duty_to_percent};
use gettextrs::gettext;
use gtk::{glib, pango};

use super::{Layout, PageContext, footer_text, legend_items, percent, source_name};
use crate::fan_chart::swatch::Swatch;
use crate::fan_chart::{ChartCurve, FanChart, Reading};
use crate::overview;

/// The widgets of one row of the list of points: one point, or a run of
/// equal points ("4–7").
struct PointRow {
    row: gtk::ListBoxRow,
    number: gtk::Label,
    temp: gtk::Label,
    duty: gtk::Label,
}

/// The parts under the chart, in two columns or in one.
struct Columns {
    grid: gtk::Grid,
    points_header: gtk::Widget,
    points: gtk::Widget,
    header_header: gtk::Widget,
    /// The header, the source and how a curve works.
    side: gtk::Widget,
    /// Two columns, once laid out.
    wide: Cell<Option<bool>>,
}

/// What the list of fans does not show about the header (D62).
struct HeaderRows {
    mode: gtk::Label,
    manual: adw::ActionRow,
    minimum_row: adw::ActionRow,
    minimum: gtk::Label,
    unverified: adw::ActionRow,
    empty: adw::ActionRow,
}

pub(super) struct CurvePage {
    pub(super) key: String,
    pub(super) page: adw::NavigationPage,
    title: adw::WindowTitle,
    /// The chart with its legends.
    chart_card: gtk::Box,
    /// In place of the chart: the curve could not be read.
    unreadable: gtk::ListBox,
    chart: FanChart,
    legend: gtk::Box,
    legend_now: gtk::Label,
    legend_target: gtk::Box,
    legend_target_label: gtk::Label,
    columns: Columns,
    points: gtk::ListBox,
    rows: RefCell<Vec<PointRow>>,
    shown: RefCell<Vec<CurvePoint>>,
    header: HeaderRows,
    source_row: adw::ActionRow,
    /// The chart labels the operating point itself: no legend.
    labelled: Rc<Cell<bool>>,
}

fn group(title: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder()
        .title(glib::markup_escape_text(title))
        .build()
}

/// A boxed list without rows yet.
fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build()
}

/// The title of `list` and its description, as AdwPreferencesGroup draws
/// them, but as a widget of its own: the page puts the headers of both
/// columns in one grid row, so that the titles stand on one line and the
/// lists start at one height, also when only one header has a description.
fn list_header(list: &gtk::ListBox, title: &str, description: Option<&str>) -> gtk::Box {
    // The spacing and margin of AdwPreferencesGroup in libadwaita 1.10.
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_bottom(12)
        .valign(gtk::Align::Start)
        .build();
    let title = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .natural_wrap_mode(gtk::NaturalWrapMode::Word)
        .css_classes(["heading"])
        .build();
    header.append(&title);
    list.update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
    if let Some(description) = description {
        let description = gtk::Label::builder()
            .label(description)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .css_classes(["body", "dimmed"])
            .build();
        header.append(&description);
        list.update_relation(&[gtk::accessible::Relation::DescribedBy(&[
            description.upcast_ref()
        ])]);
    }
    header
}

fn info_row(icon: &str, title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(title)
        .subtitle(subtitle)
        .build();
    let image = gtk::Image::from_icon_name(icon);
    image.set_accessible_role(gtk::AccessibleRole::Presentation);
    row.add_prefix(&image);
    row
}

fn set_label(label: &gtk::Label, text: &str) {
    if label.label() != text {
        label.set_label(text);
    }
}

/// The mode of a header, as its row has it.
pub(super) fn fan_mode(fan: &Fan) -> Option<Mode> {
    match &fan.row.subtitle {
        Subtitle::Header { mode, .. } => *mode,
        _ => None,
    }
}

/// The name of a mode, as the mode switch of the editor will name it, or
/// why there is none.
pub(super) fn mode_text(mode: Option<Mode>) -> String {
    match mode {
        Some(Mode::Auto) => gettext("Automatic BIOS"),
        Some(Mode::Manual) => gettext("Manual"),
        // Translators: the mode of a fan when the kernel driver gives none.
        None => gettext("The mode could not be read"),
    }
}

/// The number of a row of points (indices from 0) and what a screen
/// reader says for it: "3" and "Point 3: 80 °C, 75 %.", or for a run of
/// equal points "4–7" and "Points 4 to 7: 85 °C, 100 %.".
fn point_texts(first: usize, last: usize, temp: &str, duty: &str) -> (String, String) {
    let (from, to) = ((first + 1).to_string(), (last + 1).to_string());
    let (number, text) = if last > first {
        (
            format!("{from}–{to}"),
            // Translators: read by screen readers for a row of equal points
            // of a fan curve, e.g. "Points 4 to 7: 85 °C, 100 %.".
            gettext("Points {first} to {last}: {temperature}, {duty}.")
                .replace("{first}", &from)
                .replace("{last}", &to),
        )
    } else {
        (
            from.clone(),
            gettext("Point {number}: {temperature}, {duty}.").replace("{number}", &from),
        )
    };
    let text = text.replace("{temperature}", temp).replace("{duty}", duty);
    (number, text)
}

/// A row with a value on the right, e.g. "Mode  Automatic BIOS".
fn value_row(title: &str, subtitle: Option<&str>) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(title)
        .build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    let value = gtk::Label::builder()
        .css_classes(["numeric", "fan-soft"])
        .build();
    row.add_suffix(&value);
    (row, value)
}

/// The legend of the marks of the large chart (D60, D62): whole items
/// wrap, never a text.
fn chart_legend() -> adw::WrapBox {
    let legend = adw::WrapBox::builder()
        .child_spacing(22)
        .line_spacing(6)
        .accessible_role(gtk::AccessibleRole::Group)
        .css_classes(["fan-chart-legend"])
        .build();
    for (mark, text) in legend_items() {
        let item = gtk::Box::builder().spacing(7).build();
        item.append(&Swatch::new(mark));
        item.append(
            &gtk::Label::builder()
                .label(text)
                .css_classes(["caption", "fan-soft"])
                .build(),
        );
        legend.append(&item);
    }
    // Translators: read by screen readers for the legend under the large
    // chart of a fan curve.
    let name = gettext("Legend of the chart");
    legend
        .upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label(&name)]);
    legend
}

/// A swatch of the legend: a dot like the operating point, or a ring like
/// the target.
fn swatch(css: &str) -> gtk::Box {
    let swatch = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["fan-swatch", css])
        .build();
    swatch.set_accessible_role(gtk::AccessibleRole::Presentation);
    swatch
}

impl CurvePage {
    pub(super) fn new(key: &str) -> Self {
        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(24)
            .margin_bottom(36)
            .margin_start(12)
            .margin_end(12)
            .build();

        // The chart and its legend.
        let chart_card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["card", "fan-chart-card"])
            .build();
        let chart = FanChart::new();
        chart_card.append(&chart);
        let marks = chart_legend();
        marks.set_margin_top(10);
        marks.set_margin_start(8);
        marks.set_margin_end(8);
        chart_card.append(&marks);
        let legend = gtk::Box::builder()
            .spacing(18)
            .visible(false)
            .css_classes(["fan-legend"])
            .build();
        let now_item = gtk::Box::builder().spacing(7).build();
        now_item.append(&swatch("fan-swatch-dot"));
        let legend_now = gtk::Label::new(None);
        now_item.append(&legend_now);
        let legend_target = gtk::Box::builder().spacing(7).build();
        legend_target.append(&swatch("fan-swatch-ring"));
        let legend_target_label = gtk::Label::new(None);
        legend_target.append(&legend_target_label);
        legend.append(&now_item);
        legend.append(&legend_target);
        chart_card.append(&legend);
        content.append(&chart_card);

        // In place of the chart, when the curve could not be read.
        let unreadable = boxed_list();
        unreadable.append(&info_row(
            "help-about-symbolic",
            // Translators: on a fan's row and on its curve page when the
            // board's controller did not give the curve.
            &gettext("The curve could not be read"),
            &gettext("Baseboard tries again every 10 seconds."),
        ));
        unreadable.set_visible(false);
        content.append(&unreadable);

        // Points on the left; source and how it works on the right, or
        // under them in a narrow window (`set_layout`).
        let columns = gtk::Grid::builder()
            .column_spacing(24)
            .column_homogeneous(true)
            .margin_top(24)
            .build();
        let points = boxed_list();
        let points_header = list_header(
            &points,
            &gettext("Points"),
            // Translators: under "Points" on the curve page.
            Some(&gettext("Values read from the board's controller.")),
        );
        let points_body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Start)
            .build();
        points_body.append(&points);

        let side = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .valign(gtk::Align::Start)
            .build();

        // What the list of fans does not show (D62).
        let header_list = boxed_list();
        let header_header = list_header(
            &header_list,
            // Translators: on the curve page, the group about the fan
            // header of the board: its mode, minimum and notes.
            &gettext("Header"),
            None,
        );
        let (mode_row, mode) = value_row(&gettext("Mode"), None);
        let manual = info_row(
            "help-about-symbolic",
            // Translators: on the curve page of a fan in manual mode.
            &gettext("In manual mode the curve does not run"),
            &gettext(
                "The fan holds a fixed duty cycle, and the curve stays in the board's controller.",
            ),
        );
        let (minimum_row, minimum) = value_row(
            // Translators: on the curve page, the lowest duty cycle of a
            // header.
            &gettext("Minimum"),
            Some(&gettext("The lowest duty cycle Baseboard allows")),
        );
        let unverified = info_row(
            "help-about-symbolic",
            // Translators: on the curve page of a fan header whose channel
            // nobody has checked on this board.
            &gettext("This header was not checked on this board"),
            &gettext("Nobody has confirmed yet that this reading belongs to this header."),
        );
        let empty = info_row(
            "help-about-symbolic",
            // Translators: on the curve page of a fan that has not spun
            // since Baseboard started.
            &gettext("Not spinning since the app started"),
            &gettext("The header may be empty."),
        );
        for row in [&mode_row, &manual, &minimum_row, &unverified, &empty] {
            header_list.append(row);
        }
        side.append(&header_list);

        let source_group = group(&gettext("Temperature source"));
        // Translators: under "Temperature source" on the curve page; "it"
        // is the temperature.
        let computes = gettext("The controller computes the duty cycle from it.");
        source_group.set_description(Some(&computes));
        let source_row = adw::ActionRow::builder().use_markup(false).build();
        let thermometer = gtk::Image::from_icon_name("baseboard-temperature-symbolic");
        thermometer.set_accessible_role(gtk::AccessibleRole::Presentation);
        source_row.add_prefix(&thermometer);
        let lock = gtk::Image::builder()
            .icon_name("system-lock-screen-symbolic")
            .css_classes(["dimmed"])
            .build();
        // Translators: read by screen readers next to the temperature
        // source on the curve page.
        lock.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Cannot be changed",
        ))]);
        source_row.add_suffix(&lock);
        source_group.add(&source_row);
        side.append(&source_group);
        let how = group(&gettext("How the curve works"));
        how.add(&info_row(
            "baseboard-curve-symbolic",
            &gettext("The duty cycle changes linearly between the points"),
            &gettext("Below the first point and above the last one, their duty cycle applies."),
        ));
        how.add(&info_row(
            "baseboard-board-symbolic",
            &gettext("The board's controller runs the curve"),
            &gettext("Baseboard does not have to run for it."),
        ));
        side.append(&how);
        content.append(&columns);
        // What this version does, as under the list of fans.
        content.append(
            &gtk::Label::builder()
                .label(footer_text())
                .wrap(true)
                .justify(gtk::Justification::Center)
                .margin_top(24)
                .css_classes(["caption", "dimmed"])
                .build(),
        );

        let clamp = adw::Clamp::builder()
            .maximum_size(940)
            .tightening_threshold(700)
            .child(&content)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&clamp)
            .build();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&scrolled));
        let page = adw::NavigationPage::builder()
            .child(&view)
            .tag(format!("curve-{key}"))
            .build();

        let labelled = Rc::new(Cell::new(true));
        chart.connect_now_label(glib::clone!(
            #[strong]
            labelled,
            #[weak]
            legend,
            move |shown| {
                labelled.set(shown);
                legend.set_visible(!shown);
            }
        ));

        Self {
            key: key.to_owned(),
            page,
            title,
            chart_card,
            unreadable,
            chart,
            legend,
            legend_now,
            legend_target,
            legend_target_label,
            columns: Columns {
                grid: columns,
                points_header: points_header.upcast(),
                points: points_body.upcast(),
                header_header: header_header.upcast(),
                side: side.upcast(),
                wide: Cell::new(None),
            },
            points,
            rows: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            header: HeaderRows {
                mode,
                manual,
                minimum_row,
                minimum,
                unverified,
                empty,
            },
            source_row,
            labelled,
        }
    }

    pub(super) fn set_layout(&self, layout: Layout) {
        let columns = &self.columns;
        let wide = layout == Layout::Wide;
        if columns.wide.replace(Some(wide)) == Some(wide) {
            return;
        }
        let parts = [
            &columns.points_header,
            &columns.points,
            &columns.header_header,
            &columns.side,
        ];
        for part in parts {
            if part.parent().is_some() {
                columns.grid.remove(part);
            }
        }
        // Wide: the headers in the first row, the lists in the second.
        // Otherwise one column.
        let places = if wide {
            [(0, 0), (0, 1), (1, 0), (1, 1)]
        } else {
            [(0, 0), (0, 1), (0, 2), (0, 3)]
        };
        for (part, (column, row)) in parts.into_iter().zip(places) {
            columns.grid.attach(part, column, row, 1, 1);
        }
        columns
            .header_header
            .set_margin_top(if wide { 0 } else { 24 });
    }

    /// The values of the header. In manual mode the curve the controller
    /// keeps, with the fixed duty cycle as the point now; a curve that
    /// could not be read, in words.
    pub(super) fn update(&self, fan: &Fan, context: &PageContext) {
        let numbers = context.numbers;
        let name = overview::name(&fan.row.name, numbers);
        self.page.set_title(&name);
        self.title.set_title(&name);
        self.title.set_subtitle(&fan.header);

        let data = fan.curve();
        self.chart_card.set_visible(data.is_some());
        self.unreadable.set_visible(data.is_none());
        self.columns.points_header.set_visible(data.is_some());
        self.columns.points.set_visible(data.is_some());
        let source = data.and_then(|d| d.own_source.as_ref()).or(context.source);
        let source_name = source.map(|s| source_name(s, numbers)).unwrap_or_default();
        if let Some(data) = data {
            self.show_curve(fan, data, &name, &source_name, numbers);
        }
        self.show_header(fan);
        self.show_source(source, &source_name);
    }

    /// The chart, its legend and the points.
    fn show_curve(&self, fan: &Fan, data: &Chart, name: &str, source: &str, numbers: &Numbers) {
        self.chart.set_curve(Some(ChartCurve {
            name: name.to_owned(),
            source: source.to_owned(),
            points: data.points.clone(),
            min_percent: fan.min_percent,
            threshold: data.threshold,
            needs_full: data.needs_full,
            // In manual mode the controller keeps the curve, but it sets
            // nothing.
            in_effect: fan.chart().is_some(),
        }));
        let reading = data
            .millidegrees
            .zip(fan.duty)
            .map(|(millidegrees, duty)| Reading { millidegrees, duty });
        self.chart.set_reading(reading);

        // The legend, for when the chart has no room for its label.
        if let Some(reading) = reading {
            // Translators: in the legend under a fan curve, the operating
            // point, e.g. "Now: 46.0 °C, 38 %".
            let text = gettext("Now: {temperature}, {duty}")
                .replace(
                    "{temperature}",
                    &numbers.value(Kind::Temp, reading.millidegrees as f64 / 1000.0),
                )
                .replace("{duty}", &percent(duty_to_percent(reading.duty)));
            if self.legend_now.label() != text {
                self.legend_now.set_label(&text);
            }
        }
        let target = fan.moving_to();
        self.legend_target.set_visible(target.is_some());
        if let Some(target) = target {
            // Translators: in the legend under a fan curve, the duty cycle
            // the fan moves to, e.g. "Target: 38 %".
            self.legend_target_label.set_label(
                &gettext("Target: {duty}").replace("{duty}", &percent(duty_to_percent(target))),
            );
        }
        self.legend
            .set_visible(reading.is_some() && !self.labelled.get());
        self.show_points(&data.points);
    }

    /// The mode, the minimum and the notes of the header (D62).
    fn show_header(&self, fan: &Fan) {
        let rows = &self.header;
        let mode = fan_mode(fan);
        set_label(&rows.mode, &mode_text(mode));
        rows.manual.set_visible(mode == Some(Mode::Manual));
        let minimum = fan.min_percent > 0;
        rows.minimum_row.set_visible(minimum);
        if minimum {
            set_label(&rows.minimum, &percent(fan.min_percent));
        }
        rows.unverified.set_visible(!fan.verified);
        rows.empty.set_visible(fan.maybe_empty());
    }

    /// One row per run of equal points: the controller has a fixed number
    /// of points, and a curve with fewer levels repeats the last one, so
    /// "4–7" says it in one row.
    fn show_points(&self, points: &[CurvePoint]) {
        if *self.shown.borrow() == points {
            return;
        }
        self.shown.replace(points.to_vec());
        let runs = curve::runs(points);
        let mut rows = self.rows.borrow_mut();
        while rows.len() > runs.len() {
            if let Some(old) = rows.pop() {
                self.points.remove(&old.row);
            }
        }
        while rows.len() < runs.len() {
            let line = gtk::Box::builder()
                .spacing(10)
                .css_classes(["fan-point"])
                .build();
            let badge = gtk::Label::builder()
                .valign(gtk::Align::Center)
                .halign(gtk::Align::Start)
                .css_classes(["fan-point-number", "numeric"])
                .build();
            badge.set_accessible_role(gtk::AccessibleRole::Presentation);
            let spacer = gtk::Box::builder().hexpand(true).build();
            let value = |chars| {
                gtk::Label::builder()
                    .xalign(1.0)
                    .width_chars(chars)
                    .css_classes(["numeric"])
                    .build()
            };
            let temp = value(7);
            let duty = value(7);
            line.append(&badge);
            line.append(&spacer);
            line.append(&temp);
            line.append(&duty);
            let row = gtk::ListBoxRow::builder()
                .child(&line)
                .activatable(false)
                .build();
            self.points.append(&row);
            rows.push(PointRow {
                row,
                number: badge,
                temp,
                duty,
            });
        }
        for (row, &(first, last)) in rows.iter().zip(&runs) {
            let point = points[first];
            let temp = format!("{}{UNIT_SPACE}°C", point.temp);
            let duty = percent(duty_to_percent(point.duty));
            let (number, text) = point_texts(first, last, &temp, &duty);
            set_label(&row.number, &number);
            if last > first {
                row.number.add_css_class("wide");
            } else {
                row.number.remove_css_class("wide");
            }
            set_label(&row.temp, &temp);
            set_label(&row.duty, &duty);
            row.row
                .update_property(&[gtk::accessible::Property::Label(&text)]);
        }
    }

    fn show_source(&self, source: Option<&SourceRow>, name: &str) {
        let subtitle = source
            .map(|s| match s.source {
                Source::Temp(n) => format!("{} · temp{n}", s.chip),
                _ => s.chip.clone(),
            })
            .unwrap_or_default();
        if self.source_row.title() != name {
            self.source_row.set_title(name);
        }
        if self.source_row.subtitle().as_deref() != Some(subtitle.as_str()) {
            self.source_row.set_subtitle(&subtitle);
        }
    }
}

#[cfg(test)]
mod tests {
    use baseboard_core::fans::page::{Body, FanKind};
    use baseboard_core::overview::{Name, Row, RowId};
    use baseboard_core::profile::FanRole;

    use super::*;

    /// A header with a curve, checked on this board or not, in automatic
    /// mode.
    fn fan(verified: bool, rpm: f64, min_percent: u8) -> Fan {
        let mode = Some(Mode::Auto);
        Fan {
            key: "pump1".into(),
            role: FanRole::Pump,
            row: Row {
                id: RowId {
                    chip: "nct6687".into(),
                    device: None,
                    channel: "fan2".into(),
                },
                name: Name::Fan {
                    role: FanRole::Pump,
                    number: 1,
                },
                role: Name::Fan {
                    role: FanRole::Pump,
                    number: 1,
                },
                subtitle: Subtitle::Header {
                    header: "PUMP_SYS1".into(),
                    duty: Some(255.0),
                    verified,
                    mode,
                },
                kind: Kind::Fan,
                value: rpm,
                rating: None,
                range: Some((rpm, rpm)),
            },
            header: "PUMP_SYS1".into(),
            verified,
            min_percent,
            duty: Some(255),
            kind: FanKind::Full {
                manual: false,
                body: Body::Chart(Box::new(Chart {
                    points: vec![
                        CurvePoint {
                            temp: 40,
                            duty: 255
                        };
                        7
                    ],
                    own_source: None,
                    millidegrees: Some(50_000),
                    threshold: Some(95),
                    target: Some(255),
                    needs_full: true,
                })),
            },
        }
    }

    /// A run of equal points is one row, "4–7", read as "points 4 to 7".
    #[test]
    fn rows_of_points() {
        assert_eq!(
            point_texts(2, 2, "80\u{a0}°C", "75\u{a0}%"),
            ("3".to_owned(), "Point 3: 80\u{a0}°C, 75\u{a0}%.".to_owned())
        );
        assert_eq!(
            point_texts(3, 6, "85\u{a0}°C", "100\u{a0}%"),
            (
                "4–7".to_owned(),
                "Points 4 to 7: 85\u{a0}°C, 100\u{a0}%.".to_owned()
            )
        );
    }

    /// The list of points of the curve page: one row per run, the run with
    /// a wide number. Needs a display.
    #[test]
    fn list_of_points() {
        crate::gtk_test::run("list_of_points", || {
            let numbers = Numbers::new(".");
            let context = PageContext {
                numbers: &numbers,
                source: None,
            };
            let page = CurvePage::new("pump1");
            let mut fan = fan(true, 900.0, 0);
            let FanKind::Full {
                body: Body::Chart(chart),
                ..
            } = &mut fan.kind
            else {
                unreachable!();
            };
            chart.points = [(0, 122), (75, 124), (80, 191), (85, 255)]
                .into_iter()
                .chain([(85, 255); 3])
                .map(|(temp, duty)| CurvePoint { temp, duty })
                .collect();
            page.update(&fan, &context);
            let rows = page.rows.borrow();
            let numbers: Vec<String> = rows.iter().map(|r| r.number.label().into()).collect();
            assert_eq!(numbers, ["1", "2", "3", "4–7"]);
            assert!(rows[3].number.has_css_class("wide"));
            assert!(!rows[0].number.has_css_class("wide"));
            assert_eq!(rows[3].temp.label(), "85\u{a0}°C");
            assert_eq!(rows[3].duty.label(), "100\u{a0}%");
        });
    }

    /// A fan in manual mode opens the curve the controller keeps, with a
    /// note that it does not run now. Needs a display.
    #[test]
    fn curve_page_in_manual_mode() {
        crate::gtk_test::run("curve_page_in_manual_mode", || {
            let numbers = Numbers::new(".");
            let context = PageContext {
                numbers: &numbers,
                source: None,
            };
            let page = CurvePage::new("pump1");
            page.update(&fan(true, 900.0, 0), &context);
            assert!(!page.header.manual.is_visible());

            let mut fan = fan(true, 900.0, 0);
            if let Subtitle::Header { mode, .. } = &mut fan.row.subtitle {
                *mode = Some(Mode::Manual);
            }
            let FanKind::Full { manual, .. } = &mut fan.kind else {
                unreachable!();
            };
            *manual = true;
            page.update(&fan, &context);
            assert_eq!(page.header.mode.label(), "Manual");
            assert!(page.header.manual.is_visible());
            assert_eq!(page.rows.borrow().len(), 1);
        });
    }

    /// A curve that could not be read: the page says so in place of the
    /// chart and the points, and shows the header; once read, the chart
    /// comes back. Needs a display.
    #[test]
    fn curve_that_could_not_be_read() {
        crate::gtk_test::run("curve_that_could_not_be_read", || {
            let numbers = Numbers::new(".");
            let context = PageContext {
                numbers: &numbers,
                source: None,
            };
            let page = CurvePage::new("pump1");
            let mut unreadable = fan(false, 900.0, 30);
            unreadable.kind = FanKind::Full {
                manual: false,
                body: Body::CurveUnreadable,
            };
            page.update(&unreadable, &context);
            assert!(page.unreadable.get_visible());
            assert!(!page.chart_card.get_visible());
            assert!(!page.columns.points.get_visible());
            assert_eq!(page.title.subtitle(), "PUMP_SYS1");
            assert!(page.header.unverified.get_visible());
            assert_eq!(page.header.minimum.label(), "30\u{a0}%");

            page.update(&fan(true, 900.0, 0), &context);
            assert!(!page.unreadable.get_visible());
            assert!(page.chart_card.get_visible());
            assert!(page.columns.points.get_visible());
        });
    }

    #[test]
    fn mode_of_a_header() {
        assert_eq!(mode_text(Some(Mode::Auto)), "Automatic BIOS");
        assert_eq!(mode_text(Some(Mode::Manual)), "Manual");
        assert_eq!(mode_text(None), "The mode could not be read");
        // The mode as the Overview read it.
        assert_eq!(fan_mode(&fan(false, 0.0, 50)), Some(Mode::Auto));
    }

    /// The group "Header" says what the list of fans no longer shows
    /// (D62): the minimum where there is one, and the notes of a header
    /// nobody checked and of one that does not spin. Needs a display.
    #[test]
    fn header_group_of_the_curve_page() {
        crate::gtk_test::run("header_group_of_the_curve_page", || {
            let numbers = Numbers::new(".");
            let context = PageContext {
                numbers: &numbers,
                source: None,
            };
            let page = CurvePage::new("pump1");
            page.update(&fan(false, 0.0, 50), &context);
            let rows = &page.header;
            assert_eq!(rows.mode.label(), "Automatic BIOS");
            assert!(rows.minimum_row.is_visible());
            assert_eq!(rows.minimum.label(), "50\u{a0}%");
            assert!(rows.unverified.is_visible());
            assert!(rows.empty.is_visible());

            page.update(&fan(true, 900.0, 0), &context);
            assert!(!rows.minimum_row.is_visible());
            assert!(!rows.unverified.is_visible());
            assert!(!rows.empty.is_visible());
        });
    }
}
