// SPDX-License-Identifier: GPL-3.0-or-later

//! A row of the list of fans (D62): the name and, when there is one, the
//! state in plain words; the small chart; the speed over the duty cycle in
//! percent; an arrow where the row opens the curve details. In a narrow
//! window the numbers go into the subtitle, which then also has the width
//! of the chart. The subtitle is never broken in a sentence: it shows the
//! longest of its texts that fits on its line (`FitLine`).
//!
//! The row reads as the fan's name, with everything the list leaves out in
//! its description: the header label, speed, duty cycle, target, mode,
//! minimum and the state of the header.

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::driver::Mode;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{Numbers, UNIT_SPACE, duty_to_percent};
use gettextrs::{gettext, pgettext};
use gtk::{glib, pango};

use super::curve_page::mode_text;
use super::percent;
use super::spark::Spark;

/// A change of the duty cycle shown on screen, in points of percent;
/// smaller ones only go to screen readers (D62).
pub(super) const VISIBLE_CHANGE: u8 = 5;

/// The small chart in a wide and in a narrow window, in pixels.
const SPARK_WIDE: (i32, i32) = (192, 36);
const SPARK_NARROW: (i32, i32) = (84, 28);

/// One fan as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FanLine {
    pub key: String,
    pub name: String,
    /// The label of the header on the board, e.g. CPU_FAN1.
    pub header: String,
    /// Speed in rpm.
    pub rpm: f64,
    /// Duty cycle 0–255, where the driver reports it.
    pub duty: Option<u8>,
    /// The channel has modes (a header the driver controls).
    pub has_mode: bool,
    pub mode: Option<Mode>,
    /// The minimum duty cycle in percent (B5); 0 without one.
    pub min_percent: u8,
    /// The duty cycle the curve gives, while the fan moves to it.
    pub target: Option<u8>,
    /// Nobody checked the header on this board.
    pub unchecked: bool,
    /// No speed since the start at a duty cycle above 0.
    pub maybe_empty: bool,
    /// A CPU fan that was spinning and stands now (D65).
    pub stopped: bool,
    /// The line of the small chart, in °C and 0–255; empty without one.
    pub chart: Vec<(f64, f64)>,
    /// The point now, in °C and 0–255.
    pub now: Option<(f64, f64)>,
    /// What a screen reader says for the chart.
    pub chart_description: Option<String>,
    /// The row opens the curve details.
    pub opens: bool,
}

impl FanLine {
    /// The fan moves to its target by at least `VISIBLE_CHANGE` points:
    /// `Some(true)` up, `Some(false)` down.
    pub(super) fn visible_move(&self) -> Option<bool> {
        let (target, duty) = (self.target?, self.duty?);
        let (target, duty) = (duty_to_percent(target), duty_to_percent(duty));
        (target.abs_diff(duty) >= VISIBLE_CHANGE).then_some(target > duty)
    }

    /// The state in plain words, when there is one: a stopped CPU fan, an
    /// empty header, manual mode, or a visible move to the target.
    pub(super) fn state(&self) -> Option<String> {
        if self.stopped {
            // Translators: under the CPU fan on the Fans page when it was
            // spinning before and stands now (D65).
            return Some(pgettext("fan state", "Stopped"));
        }
        if self.maybe_empty {
            // Translators: under a fan on the Fans page when it has not
            // spun since Baseboard started; one sentence.
            return Some(gettext("Not spinning – the header may be empty."));
        }
        if self.mode == Some(Mode::Manual) {
            // Translators: under a fan on the Fans page, its mode.
            return Some(pgettext("fan state", "Manual"));
        }
        self.visible_move().map(|up| {
            if up {
                // Translators: under a fan on the Fans page while its duty
                // cycle rises to the one of its curve.
                gettext("Speeding up")
            } else {
                // Translators: under a fan on the Fans page while its duty
                // cycle falls to the one of its curve.
                gettext("Slowing down")
            }
        })
    }

    /// The texts of the subtitle, longest first: in a narrow window the
    /// speed and duty cycle with the state, then the state alone.
    pub(super) fn subtitles(&self, narrow: bool, numbers: &Numbers) -> Vec<String> {
        let state = self.state();
        if !narrow {
            return state.into_iter().collect();
        }
        let mut values = numbers.value(Kind::Fan, self.rpm);
        if let Some(duty) = self.duty {
            values = format!("{values} · {}", percent(duty_to_percent(duty)));
        }
        match state {
            Some(state) => vec![format!("{values} · {state}"), state],
            None => vec![values],
        }
    }

    /// The description of the row for screen readers: what the list does
    /// not show, or shows shortened.
    pub(super) fn description(&self, numbers: &Numbers) -> String {
        let mut parts = Vec::new();
        if self.stopped {
            parts.push(crate::alert::cpu_fan_stopped());
        }
        parts.extend([self.header.clone(), numbers.value(Kind::Fan, self.rpm)]);
        if let Some(duty) = self.duty {
            parts.push(gettext("duty {duty}").replace("{duty}", &percent(duty_to_percent(duty))));
        }
        if let Some(target) = self.target.filter(|_| self.duty.is_some()) {
            parts.push(
                // Translators: read by screen readers for a fan, after its
                // duty cycle, e.g. "the controller changes it gradually to
                // 100 %".
                gettext("the controller changes it gradually to {duty}")
                    .replace("{duty}", &percent(duty_to_percent(target))),
            );
        }
        if self.has_mode {
            parts.push(gettext("mode: {mode}").replace("{mode}", &mode_text(self.mode)));
            if self.min_percent > 0 {
                parts.push(gettext("minimum {duty}").replace("{duty}", &percent(self.min_percent)));
            }
        }
        if self.unchecked {
            parts.push(gettext("This header was not checked on this board."));
        }
        if self.maybe_empty {
            parts.push(gettext(
                "Not spinning since the app started: the header may be empty.",
            ));
        }
        parts.join(", ")
    }
}

/// Whether some fan of the list visibly speeds up to its curve.
pub(super) fn speeding_up(lines: &[FanLine]) -> bool {
    lines.iter().any(|line| line.visible_move() == Some(true))
}

/// The first of `widths` that fits in `width`, or the last one.
pub(super) fn first_fitting(widths: &[i32], width: i32) -> usize {
    widths
        .iter()
        .position(|&w| w <= width)
        .unwrap_or(widths.len().saturating_sub(1))
}

// ---------- a line that shows the longest text that fits ----------

mod fit_imp {
    use super::*;

    #[derive(Default)]
    pub struct FitLine {
        pub(super) child: OnceCell<gtk::Inscription>,
        pub(super) texts: RefCell<Vec<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FitLine {
        const NAME: &'static str = "BaseboardFitLine";
        type Type = super::FitLine;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for FitLine {
        fn dispose(&self) {
            if let Some(child) = self.child.get() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for FitLine {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match (orientation, self.child.get()) {
                // It takes the width it is given and fits its text into it.
                (gtk::Orientation::Horizontal, _) | (_, None) => (0, 0, -1, -1),
                (_, Some(child)) => child.measure(orientation, for_size),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let obj = self.obj();
            obj.pick(width);
            if let Some(child) = self.child.get() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    /// One line of text that shows the first of its texts that fits its
    /// width, never a broken one; with none fitting, the last one with an
    /// ellipsis. A new choice only redraws: the line is as high as one
    /// line of its font whatever the text.
    pub struct FitLine(ObjectSubclass<fit_imp::FitLine>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FitLine {
    pub fn new(css: &[&str]) -> Self {
        let line: Self = glib::Object::new();
        let child = gtk::Inscription::builder()
            .xalign(0.0)
            .min_chars(0)
            .nat_chars(0)
            .text_overflow(gtk::InscriptionOverflow::EllipsizeEnd)
            .css_classes(css)
            .build();
        child.set_parent(&line);
        let _ = line.imp().child.set(child);
        line
    }

    /// The texts, longest first.
    pub fn set_texts(&self, texts: Vec<String>) {
        if *self.imp().texts.borrow() == texts {
            return;
        }
        self.imp().texts.replace(texts);
        self.pick(self.width());
    }

    /// The text shown now.
    pub fn text(&self) -> String {
        self.imp()
            .child
            .get()
            .and_then(|c| c.text())
            .map(|t| t.to_string())
            .unwrap_or_default()
    }

    fn pick(&self, width: i32) {
        let Some(child) = self.imp().child.get() else {
            return;
        };
        let texts = self.imp().texts.borrow();
        let widths: Vec<i32> = texts
            .iter()
            .map(|text| child.create_pango_layout(Some(text)).pixel_size().0)
            .collect();
        let text = texts.get(first_fitting(&widths, width)).map(String::as_str);
        if child.text().as_deref() != text {
            child.set_text(text);
        }
    }
}

// ---------- the row ----------

/// The widest text of `inscription` for `markup`, in its font with tabular
/// digits as the `numeric` style draws them.
fn text_width(inscription: &gtk::Inscription, markup: &str) -> i32 {
    let layout = inscription.create_pango_layout(None);
    layout.set_markup(markup);
    let attributes = layout.attributes().unwrap_or_default();
    attributes.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attributes));
    let (ink, logical) = layout.pixel_extents();
    (ink.x() + ink.width()).max(logical.x() + logical.width())
}

/// The speed in large type, its unit dimmed.
fn rpm_markup(numbers: &Numbers, rpm: f64) -> String {
    let speed = numbers.value(Kind::Fan, rpm);
    let (number, unit) = speed.split_once(UNIT_SPACE).unwrap_or((&speed, ""));
    format!(
        "<b>{}</b><span alpha=\"50%\">{UNIT_SPACE}{}</span>",
        glib::markup_escape_text(number),
        glib::markup_escape_text(unit)
    )
}

pub(super) struct FanRow {
    pub(super) row: gtk::ListBoxRow,
    grid: gtk::Grid,
    /// The name over the state, in the middle of a wide row.
    texts: gtk::Box,
    name: gtk::Label,
    sub: FitLine,
    spark: Spark,
    values: gtk::Box,
    rpm: gtk::Inscription,
    pct: gtk::Inscription,
    chevron: gtk::Image,
    narrow: Cell<Option<bool>>,
    line: RefCell<FanLine>,
}

impl FanRow {
    pub(super) fn new(line: &FanLine, numbers: &Numbers, narrow: bool) -> Self {
        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(2)
            .margin_top(9)
            .margin_bottom(9)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["fan-row"])
            .build();
        let name = gtk::Label::builder()
            .label(&line.name)
            .xalign(0.0)
            .hexpand(true)
            .single_line_mode(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["fan-row-name"])
            .build();
        let sub = FitLine::new(&["fan-row-sub"]);
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let spark = Spark::new(SPARK_WIDE.0, SPARK_WIDE.1);
        spark.set_valign(gtk::Align::Center);
        let rpm = gtk::Inscription::builder()
            .xalign(1.0)
            .min_chars(0)
            .nat_chars(0)
            .css_classes(["fan-row-rpm", "numeric"])
            .build();
        let pct = gtk::Inscription::builder()
            .xalign(1.0)
            .min_chars(0)
            .nat_chars(0)
            .css_classes(["fan-row-pct", "fan-soft", "numeric"])
            .build();
        let values = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .build();
        values.append(&rpm);
        values.append(&pct);
        let chevron = gtk::Image::builder()
            .icon_name("go-next-symbolic")
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["fan-soft"])
            .build();

        let row = gtk::ListBoxRow::builder().child(&grid).build();
        let widgets = Self {
            row,
            grid,
            texts,
            name,
            sub,
            spark,
            values,
            rpm,
            pct,
            chevron,
            narrow: Cell::new(None),
            line: RefCell::new(line.clone()),
        };
        widgets.set_opens(line);
        widgets.set_narrow(narrow, numbers);
        widgets.update(line, numbers);
        widgets
    }

    fn set_opens(&self, line: &FanLine) {
        self.row.set_activatable(line.opens);
        if line.opens {
            // The target before the name, as in the Overview's summary.
            self.row
                .set_action_target_value(Some(&line.key.to_variant()));
            self.row.set_action_name(Some("win.show-curve"));
        }
        // Kept in place, so that the numbers of all rows line up.
        self.chevron.set_opacity(if line.opens { 1.0 } else { 0.0 });
    }

    /// Wide: name over state | chart | speed over percent | arrow, all in
    /// the middle of the row. Narrow: name | chart | arrow, and the
    /// subtitle under the name and the chart, with the numbers in it.
    pub(super) fn set_narrow(&self, narrow: bool, numbers: &Numbers) {
        if self.narrow.replace(Some(narrow)) == Some(narrow) {
            return;
        }
        for widget in [
            self.name.upcast_ref::<gtk::Widget>(),
            self.sub.upcast_ref(),
            self.texts.upcast_ref(),
            self.spark.upcast_ref(),
            self.values.upcast_ref(),
            self.chevron.upcast_ref(),
        ] {
            if let Some(parent) = widget.parent() {
                if parent == *self.texts.upcast_ref::<gtk::Widget>() {
                    self.texts.remove(widget);
                } else {
                    self.grid.remove(widget);
                }
            }
        }
        let (width, height) = if narrow { SPARK_NARROW } else { SPARK_WIDE };
        self.spark.set_size(width, height);
        if narrow {
            self.grid.attach(&self.name, 0, 0, 1, 1);
            self.grid.attach(&self.spark, 1, 0, 1, 1);
            self.grid.attach(&self.sub, 0, 1, 2, 1);
            self.grid.attach(&self.chevron, 2, 0, 1, 2);
        } else {
            self.texts.append(&self.name);
            self.texts.append(&self.sub);
            self.grid.attach(&self.texts, 0, 0, 1, 1);
            self.grid.attach(&self.spark, 1, 0, 1, 1);
            self.grid.attach(&self.values, 2, 0, 1, 1);
            self.grid.attach(&self.chevron, 3, 0, 1, 1);
        }
        let line = self.line.borrow().clone();
        self.show_subtitle(&line, numbers);
    }

    fn show_subtitle(&self, line: &FanLine, numbers: &Numbers) {
        let narrow = self.narrow.get().unwrap_or(false);
        let texts = line.subtitles(narrow, numbers);
        // Without a subtitle the name stands alone in the middle of the
        // row; a narrow row always has one, with the numbers.
        self.sub.set_visible(!texts.is_empty());
        self.sub.set_texts(texts);
    }

    pub(super) fn update(&self, line: &FanLine, numbers: &Numbers) {
        if self.name.label() != line.name {
            self.name.set_label(&line.name);
        }
        self.show_subtitle(line, numbers);
        // A stopped CPU fan says so in the color of warnings, and in words.
        if line.stopped != self.sub.has_css_class("warning") {
            if line.stopped {
                self.sub.add_css_class("warning");
            } else {
                self.sub.remove_css_class("warning");
            }
        }
        // As wide as four digits, so that the chart does not move when the
        // speed gets a digit; measured here, in the font of the window's
        // style, which a new row does not have yet.
        let widest = text_width(&self.rpm, &rpm_markup(numbers, 8888.0));
        if self.rpm.width_request() != widest {
            self.rpm.set_width_request(widest);
        }
        // The inscription keeps the text without markup; compare that.
        let speed = numbers.value(Kind::Fan, line.rpm);
        if self.rpm.text().as_deref() != Some(speed.as_str()) {
            self.rpm.set_markup(Some(&rpm_markup(numbers, line.rpm)));
        }
        let still = line.rpm == 0.0;
        if still != self.rpm.has_css_class("fan-still") {
            if still {
                self.rpm.add_css_class("fan-still");
            } else {
                self.rpm.remove_css_class("fan-still");
            }
        }
        let pct = line
            .duty
            .map(|d| percent(duty_to_percent(d)))
            .unwrap_or_default();
        if self.pct.text().as_deref() != Some(pct.as_str()) {
            self.pct.set_text(Some(&pct));
        }
        // Without a duty cycle the speed stands in the middle of the row.
        self.pct.set_visible(line.duty.is_some());
        self.spark.set_curve(line.chart.clone());
        self.spark.set_now(line.now);
        match &line.chart_description {
            Some(text) => {
                self.spark.set_accessible_role(gtk::AccessibleRole::Img);
                self.spark
                    .update_property(&[gtk::accessible::Property::Label(text)]);
            }
            None => self
                .spark
                .set_accessible_role(gtk::AccessibleRole::Presentation),
        }
        self.row.update_property(&[
            gtk::accessible::Property::Label(&line.name),
            gtk::accessible::Property::Description(&line.description(numbers)),
        ]);
        self.line.replace(line.clone());
    }

    /// The text of the subtitle shown now (tests).
    #[cfg(test)]
    pub(super) fn subtitle(&self) -> String {
        if self.sub.is_visible() {
            self.sub.text()
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pump of MS-7E61: no speed at 100 %, not checked.
    fn pump() -> FanLine {
        FanLine {
            key: "pump1".into(),
            name: "Pump\u{a0}1".into(),
            header: "PUMP_SYS1".into(),
            rpm: 0.0,
            duty: Some(255),
            has_mode: true,
            mode: Some(Mode::Auto),
            min_percent: 50,
            target: None,
            unchecked: true,
            maybe_empty: true,
            stopped: false,
            chart: vec![(40.0, 255.0)],
            now: Some((50.0, 255.0)),
            chart_description: None,
            opens: true,
        }
    }

    /// The CPU fan at 85 °C: 78 % moving to 100 %.
    fn cpu() -> FanLine {
        FanLine {
            key: "cpu1".into(),
            name: "CPU fan\u{a0}1".into(),
            header: "CPU_FAN1".into(),
            rpm: 1520.0,
            duty: Some(200),
            target: Some(255),
            unchecked: false,
            maybe_empty: false,
            min_percent: 30,
            ..pump()
        }
    }

    #[test]
    fn states_in_plain_words() {
        assert_eq!(
            pump().state().as_deref(),
            Some("Not spinning – the header may be empty.")
        );
        assert_eq!(cpu().state().as_deref(), Some("Speeding up"));
        // A change under 5 points only goes to screen readers.
        let small = FanLine {
            duty: Some(108),
            target: Some(109),
            ..cpu()
        };
        assert_eq!(small.visible_move(), None);
        assert_eq!(small.state(), None);
        let down = FanLine {
            duty: Some(200),
            target: Some(150),
            ..cpu()
        };
        assert_eq!(down.state().as_deref(), Some("Slowing down"));
        let manual = FanLine {
            mode: Some(Mode::Manual),
            target: None,
            ..cpu()
        };
        assert_eq!(manual.state().as_deref(), Some("Manual"));
        // A stopped CPU fan comes first (D65).
        let stopped = FanLine {
            rpm: 0.0,
            stopped: true,
            ..cpu()
        };
        assert_eq!(stopped.state().as_deref(), Some("Stopped"));
        assert!(
            stopped
                .description(&Numbers::new("."))
                .starts_with("CPU fan stopped, CPU_FAN1, 0\u{a0}rpm")
        );
        // "Fans are speeding up" only for a fan that goes up.
        assert!(speeding_up(&[pump(), cpu()]));
        assert!(!speeding_up(&[pump(), small, down]));
    }

    /// Wide: the state alone; narrow: the numbers with the state, then the
    /// state alone.
    #[test]
    fn subtitles_wide_and_narrow() {
        let numbers = Numbers::new(".");
        assert_eq!(cpu().subtitles(false, &numbers), ["Speeding up"]);
        assert_eq!(
            cpu().subtitles(true, &numbers),
            ["1520\u{a0}rpm · 78\u{a0}% · Speeding up", "Speeding up"]
        );
        assert_eq!(
            pump().subtitles(true, &numbers),
            [
                "0\u{a0}rpm · 100\u{a0}% · Not spinning – the header may be empty.",
                "Not spinning – the header may be empty."
            ]
        );
        let quiet = FanLine {
            target: None,
            ..cpu()
        };
        assert!(quiet.subtitles(false, &numbers).is_empty());
        assert_eq!(
            quiet.subtitles(true, &numbers),
            ["1520\u{a0}rpm · 78\u{a0}%"]
        );
        // A header without control and without a duty cycle.
        let aux = FanLine {
            duty: None,
            has_mode: false,
            target: None,
            maybe_empty: false,
            ..pump()
        };
        assert_eq!(aux.subtitles(true, &numbers), ["0\u{a0}rpm"]);
    }

    /// The description says everything the row leaves out.
    #[test]
    fn description_for_screen_readers() {
        let numbers = Numbers::new(".");
        assert_eq!(
            cpu().description(&numbers),
            "CPU_FAN1, 1520\u{a0}rpm, duty 78\u{a0}%, the controller changes it gradually \
             to 100\u{a0}%, mode: Automatic BIOS, minimum 30\u{a0}%"
        );
        assert_eq!(
            pump().description(&numbers),
            "PUMP_SYS1, 0\u{a0}rpm, duty 100\u{a0}%, mode: Automatic BIOS, minimum 50\u{a0}%, \
             This header was not checked on this board., Not spinning since the app started: \
             the header may be empty."
        );
    }

    /// Tab goes from row to row, and a row with a curve opens it (Enter
    /// activates a row) through the window's action, with the key of its
    /// fan; a row without a curve opens nothing. Needs a display.
    #[test]
    fn rows_open_the_curve_with_the_keyboard() {
        crate::gtk_test::run("rows_open_the_curve_with_the_keyboard", || {
            use std::rc::Rc;

            use gtk::gio;

            let numbers = Numbers::new(".");
            let opened = Rc::new(RefCell::new(Vec::<String>::new()));
            let action = gio::SimpleAction::new("show-curve", Some(glib::VariantTy::STRING));
            let seen = opened.clone();
            action.connect_activate(move |_, key| {
                if let Some(key) = key.and_then(|k| k.get::<String>()) {
                    seen.borrow_mut().push(key);
                }
            });
            let group = gio::SimpleActionGroup::new();
            group.add_action(&action);

            let aux = FanLine {
                key: "fan4".into(),
                opens: false,
                chart: Vec::new(),
                ..pump()
            };
            let rows: Vec<FanRow> = [cpu(), aux]
                .iter()
                .map(|line| FanRow::new(line, &numbers, false))
                .collect();
            let list = gtk::ListBox::new();
            for row in &rows {
                list.append(&row.row);
            }
            let window = gtk::Window::new();
            window.insert_action_group("win", Some(&group));
            window.set_child(Some(&list));

            // Tab goes from row to row while the list lets it (the default
            // of GtkListBox); a window that is not active cannot show it.
            assert_eq!(list.tab_behavior(), gtk::ListTabBehavior::All);
            assert!(rows.iter().all(|row| row.row.is_focusable()));

            assert!(rows[0].row.is_activatable());
            rows[0].row.activate();
            assert!(!rows[1].row.is_activatable());
            rows[1].row.activate();
            assert_eq!(*opened.borrow(), ["cpu1"]);
            window.destroy();
        });
    }

    #[test]
    fn first_text_that_fits() {
        assert_eq!(first_fitting(&[300, 200], 320), 0);
        assert_eq!(first_fitting(&[300, 200], 250), 1);
        // None fits: the last one, with an ellipsis.
        assert_eq!(first_fitting(&[300, 200], 150), 1);
        assert_eq!(first_fitting(&[], 150), 0);
    }

    /// A narrow row of the pump at the width of a 400 px window: the
    /// subtitle with the numbers does not fit, so it is the sentence alone,
    /// on one line. Needs a display.
    #[test]
    fn narrow_pump_row_keeps_its_sentence() {
        crate::gtk_test::run("narrow_pump_row_keeps_its_sentence", || {
            let numbers = Numbers::new(".");
            let row = FanRow::new(&pump(), &numbers, true);
            // The list in a 400 px window: 376 px.
            let width = 376;
            let (_, height, _, _) = row.row.measure(gtk::Orientation::Vertical, width);
            row.row.allocate(width, height, -1, None);
            assert_eq!(row.subtitle(), "Not spinning – the header may be empty.");
            // Wide, in the list of 600 px: the sentence alone.
            row.set_narrow(false, &numbers);
            let (_, height, _, _) = row.row.measure(gtk::Orientation::Vertical, 600);
            row.row.allocate(600, height, -1, None);
            assert_eq!(row.subtitle(), "Not spinning – the header may be empty.");
        });
    }
}
