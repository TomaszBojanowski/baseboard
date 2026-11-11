// SPDX-License-Identifier: GPL-3.0-or-later

//! The curve of one header as a subpage of the Fans page, in the preview
//! of version 0.2.0 (docs/PROJEKT.md §7 M2, M2.4, makiety
//! `wentylatory-krzywa.html?stan=podglad`): the large chart, a legend
//! for what the chart could not label, the points as read from the
//! board's controller, the temperature source and how a curve works.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use baseboard_core::fans::curve;
use baseboard_core::fans::driver::{CurvePoint, Source};
use baseboard_core::fans::page::{Card, SourceRow};
use baseboard_core::sensors::Kind;
use baseboard_core::units::{UNIT_SPACE, duty_to_percent};
use gettextrs::{gettext, ngettext};
use gtk::{glib, pango};

use super::{Layout, PageContext, percent, source_name};
use crate::fan_chart::{ChartCurve, FanChart, Reading, Size};
use crate::overview;

/// The widgets of one row of the list of points.
struct PointRow {
    row: gtk::ListBoxRow,
    temp: gtk::Label,
    duty: gtk::Label,
}

/// The parts under the chart, in two columns or in one.
struct Columns {
    grid: gtk::Grid,
    points_header: gtk::Widget,
    points: gtk::Widget,
    source_header: gtk::Widget,
    /// The source list and how a curve works.
    side: gtk::Widget,
    /// Two columns, once laid out.
    wide: Cell<Option<bool>>,
}

pub(super) struct CurvePage {
    pub(super) key: String,
    pub(super) page: adw::NavigationPage,
    title: adw::WindowTitle,
    notice: gtk::Label,
    chart: FanChart,
    legend: gtk::Box,
    legend_now: gtk::Label,
    legend_target: gtk::Box,
    legend_target_label: gtk::Label,
    columns: Columns,
    points: gtk::ListBox,
    rows: RefCell<Vec<PointRow>>,
    shown: RefCell<Vec<CurvePoint>>,
    repeats: gtk::Label,
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

        // What this version does with curves.
        let notice_box = gtk::Box::builder()
            .spacing(10)
            .margin_bottom(14)
            .css_classes(["fan-notice"])
            .build();
        let icon = gtk::Image::from_icon_name("help-about-symbolic");
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        let notice = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .build();
        notice_box.append(&icon);
        notice_box.append(&notice);
        content.append(&notice_box);

        // The chart and its legend.
        let chart_card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["card", "fan-chart-card"])
            .build();
        let chart = FanChart::new(Size::Large, false);
        chart_card.append(&chart);
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
            // Translators: under "Points" on the curve page in the preview.
            Some(&gettext("Values read from the board's controller.")),
        );
        let points_body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Start)
            .build();
        points_body.append(&points);
        let repeats = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .margin_top(10)
            .margin_start(2)
            .margin_end(2)
            .visible(false)
            .css_classes(["caption", "dimmed"])
            .build();
        points_body.append(&repeats);

        let side = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .valign(gtk::Align::Start)
            .build();
        let source_list = boxed_list();
        let source_header = list_header(
            &source_list,
            &gettext("Temperature source"),
            // Translators: under "Temperature source" on the curve page;
            // "it" is the temperature. One line in two columns.
            Some(&gettext("The controller computes the duty from it.")),
        );
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
        source_list.append(&source_row);
        side.append(&source_list);
        let how = group(&gettext("How the curve works"));
        how.add(&info_row(
            "baseboard-curve-symbolic",
            &gettext("The duty cycle changes linearly between the points"),
            &gettext("Below the first point and above the last one their duty cycle holds."),
        ));
        how.add(&info_row(
            "baseboard-board-symbolic",
            &gettext("The board's controller runs the curve"),
            &gettext("Neither the app nor the helper has to run for it."),
        ));
        side.append(&how);
        content.append(&columns);

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
            notice,
            chart,
            legend,
            legend_now,
            legend_target,
            legend_target_label,
            columns: Columns {
                grid: columns,
                points_header: points_header.upcast(),
                points: points_body.upcast(),
                source_header: source_header.upcast(),
                side: side.upcast(),
                wide: Cell::new(None),
            },
            points,
            rows: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            repeats,
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
            &columns.source_header,
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
            .source_header
            .set_margin_top(if wide { 0 } else { 24 });
    }

    /// The values of the header's card; `note` says what this version does
    /// with curves.
    pub(super) fn update(&self, card: &Card, context: &PageContext, note: &str) {
        let numbers = context.numbers;
        let Some(data) = card.chart() else {
            return;
        };
        let name = overview::name(&card.row.name, numbers);
        self.page.set_title(&name);
        self.title.set_title(&name);
        // Translators: the subtitle of the curve page in the preview,
        // e.g. "SYS_FAN1 · preview".
        self.title
            .set_subtitle(&gettext("{header} · preview").replace("{header}", &card.header));
        if self.notice.label() != note {
            self.notice.set_label(note);
        }

        let source = data.own_source.as_ref().or(context.source);
        let source_name = source.map(|s| source_name(s, numbers)).unwrap_or_default();
        self.chart.set_curve(Some(ChartCurve {
            name: name.clone(),
            source: source_name.clone(),
            points: data.points.clone(),
            min_percent: card.min_percent,
            threshold: data.threshold,
            needs_full: data.needs_full,
        }));
        let reading = data
            .millidegrees
            .zip(card.duty)
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
        let target = card.moving_to();
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
        self.show_source(source, &source_name);
    }

    fn show_points(&self, points: &[CurvePoint]) {
        if *self.shown.borrow() == points {
            return;
        }
        self.shown.replace(points.to_vec());
        let mut rows = self.rows.borrow_mut();
        while rows.len() > points.len() {
            if let Some(old) = rows.pop() {
                self.points.remove(&old.row);
            }
        }
        while rows.len() < points.len() {
            let number = rows.len() + 1;
            let line = gtk::Box::builder()
                .spacing(10)
                .css_classes(["fan-point"])
                .build();
            let badge = gtk::Label::builder()
                .label(number.to_string())
                .valign(gtk::Align::Center)
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
            rows.push(PointRow { row, temp, duty });
        }
        for (i, (row, point)) in rows.iter().zip(points).enumerate() {
            let temp = format!("{}{UNIT_SPACE}°C", point.temp);
            let duty = percent(duty_to_percent(point.duty));
            row.temp.set_label(&temp);
            row.duty.set_label(&duty);
            let text = gettext("Point {number}: {temperature}, {duty}.")
                .replace("{number}", &(i + 1).to_string())
                .replace("{temperature}", &temp)
                .replace("{duty}", &duty);
            row.row
                .update_property(&[gtk::accessible::Property::Label(&text)]);
        }

        // Points repeated at the end, as the BIOS fills a curve with fewer
        // levels.
        let repeated = curve::runs(points)
            .into_iter()
            .find(|&(first, last)| last > first && last + 1 == points.len());
        self.repeats.set_visible(repeated.is_some());
        if let Some((first, last)) = repeated {
            let same = if last == first + 1 {
                // Translators: under the list of points of a curve, e.g.
                // "Point 7 is the same as point 6."
                gettext("Point {last} is the same as point {first}.")
                    .replace("{last}", &(last + 1).to_string())
            } else {
                // Translators: under the list of points of a curve, e.g.
                // "Points 5–7 are the same as point 4."
                gettext("Points {from}–{last} are the same as point {first}.")
                    .replace("{from}", &(first + 2).to_string())
                    .replace("{last}", &(last + 1).to_string())
            }
            .replace("{first}", &(first + 1).to_string());
            let count = u32::try_from(points.len()).unwrap_or(u32::MAX);
            let why = ngettext(
                "The controller has {} point, and a curve with fewer levels repeats the last one.",
                "The controller has {} points, and a curve with fewer levels repeats the last one.",
                count,
            )
            .replace("{}", &count.to_string());
            self.repeats.set_label(&format!("{same} {why}"));
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
