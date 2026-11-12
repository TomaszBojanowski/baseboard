// SPDX-License-Identifier: GPL-3.0-or-later

//! The Fans page (docs/PROJEKT.md §5.5.3, §5.5.4, §7 M2, M2.4, D62) of
//! version 0.2.0: what the board's controller does with the fans, without
//! changing anything, in plain words. On top the temperature the curves
//! follow, large; under it a list of the fans as wide as in GNOME Settings,
//! a row each with a small chart. The model comes from `core::fans::page`;
//! this module gives it texts and widgets. Rows are made again only when
//! the fans change (another driver after a reload, a header turned off),
//! otherwise their values are set. Samples come from the window's sensor
//! reader, which reads the fan channels only while this page is shown.

mod curve_page;
mod row;
mod spark;

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::driver::{Mode, Source};
use baseboard_core::fans::page::{self, Card, CardKind, Note, Page, SourceRow};
use baseboard_core::overview::{Name, Row, Subtitle, Visibility};
use baseboard_core::profile::{FanRole, Profile, TempRole};
use baseboard_core::units::{Numbers, UNIT_SPACE};
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config;
use crate::fan_chart::swatch::Mark;
use crate::fan_chart::{self, ChartCurve, Reading};
use crate::overview;
use crate::sampler::Sample;
use curve_page::{CurvePage, card_mode};
use row::{FanLine, FanRow};

/// How wide the page is (fans.ui): the curve page in two columns or one;
/// the rows of the list wide or narrow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Layout {
    #[default]
    Wide,
    OneColumn,
    Narrow,
}

/// What a card and the curve page need besides their card.
pub(crate) struct PageContext<'a> {
    pub numbers: &'a Numbers,
    /// The source most curves follow.
    pub source: Option<&'a SourceRow>,
}

/// A whole percent with its unit, e.g. "30 %".
pub(crate) fn percent(value: u8) -> String {
    format!("{value}{UNIT_SPACE}%")
}

/// The name of a temperature source, as on the Overview.
pub(crate) fn source_name(row: &SourceRow, numbers: &Numbers) -> String {
    if let Some(reading) = &row.row {
        return overview::name(&reading.name, numbers);
    }
    if let Some(temp) = &row.temp {
        return overview::name(&Name::Temp(temp.role), numbers);
    }
    match row.source {
        Source::Temp(input) => format!("temp{input}"),
        // Translators: the source of a fan curve when it is a temperature
        // the kernel driver does not report.
        Source::Unreported => gettext("A temperature the driver does not report"),
        // Translators: the source of a fan curve when the controller
        // follows no temperature.
        Source::Mask(0) => gettext("No temperature"),
        Source::Mask(mask) => {
            let list: Vec<String> = (0..32)
                .filter(|bit| mask & (1 << bit) != 0)
                .map(|bit| format!("temp{}", bit + 1))
                .collect();
            // Translators: the source of a fan curve that follows several
            // temperatures at once; {list} is e.g. "temp1, temp2".
            gettext("Several temperatures: {list}").replace("{list}", &list.join(", "))
        }
    }
}

/// What the large temperature on top is (D62): "CPU" for the processor's
/// temperature of the profile, otherwise its name as on the Overview.
fn temperature_name(row: &SourceRow, numbers: &Numbers) -> String {
    match row.temp.as_ref().map(|t| t.role) {
        // Translators: above the large temperature on the Fans page, the
        // temperature of the processor.
        Some(TempRole::Cpu) => gettext("CPU"),
        _ => source_name(row, numbers),
    }
}

/// The note of the curve page in version 0.2.0.
fn preview_note() -> String {
    gettext("This version of Baseboard shows the fan settings without changing them.")
}

// The section of the project's website on fan control in the kernel, in
// the language of the app (D60). No doc comment right above: xgettext would
// take it for translators.

fn kernel_url() -> String {
    let website = config::WEBSITE;
    // Translators: the part of the project's website on fan control in the
    // kernel, after its address: "/#kernel" on the English page,
    // "/pl/#jadro" on the Polish one. Keep "/#kernel" for a language
    // without its own page.
    let section = gettext("/#kernel");
    format!("{website}{section}")
}

/// Under the temperature on a driver without control (D62): what the
/// system can do, with a link to the kernel section of the website.
fn kernel_markup() -> String {
    let text = gettext(
        // Translators: under the large temperature on the Fans page when the
        // kernel driver can only read the fans.
        "This system can only read the fans for now. Fan control is coming in a future Linux \
         version.",
    );
    let link = gettext(
        // Translators: a link after the sentence about the kernel on the
        // Fans page; it opens the website's section on fan control in the
        // kernel.
        "Kernel status",
    );
    format!(
        "{} <a href=\"{}\">{}</a>",
        glib::markup_escape_text(&text),
        glib::markup_escape_text(&kernel_url()),
        glib::markup_escape_text(&link)
    )
}

/// The footer of the list in version 0.2.0 (D62).
fn footer_text() -> String {
    keep_last_words(
        &gettext("For now, Baseboard only shows the fan settings from the BIOS."),
        3,
    )
}

/// `text` with its last `count` words held together by no-break spaces,
/// so that a wrapped line never leaves fewer of them alone on the next
/// line.
fn keep_last_words(text: &str, count: usize) -> String {
    let spaces: Vec<usize> = text.match_indices(' ').map(|(i, _)| i).collect();
    let from = spaces.len().saturating_sub(count.saturating_sub(1));
    let mut kept = String::with_capacity(text.len() + count);
    let mut start = 0;
    for &space in &spaces[from..] {
        kept.push_str(&text[start..space]);
        kept.push('\u{a0}');
        start = space + 1;
    }
    kept.push_str(&text[start..]);
    kept
}

/// The legend of the large chart on the curve page (D60, D62): each mark
/// with its name. A name does not wrap, so the legend breaks between whole
/// items.
fn legend_items() -> [(Mark, String); 4] {
    [
        // Translators: in the legend of the fan charts, the hatched zone
        // below the lowest duty cycle of a header.
        (Mark::BelowMinimum, gettext("below the header's minimum")),
        // Translators: in the legend of the fan charts, the red dashed line.
        (Mark::Threshold, gettext("emergency threshold")),
        // Translators: in the legend of the fan charts, the green dot.
        (Mark::Now, gettext("now: temperature and duty cycle")),
        // Translators: in the legend of the fan charts, the dashed line of
        // a curve before its first point and after its last one.
        (Mark::Ends, gettext("curve extension")),
    ]
}

/// A row of the list for a card of the model. `temp` is the temperature
/// on top, in °C, for the point of a fan in manual mode.
fn card_line(card: &Card, context: &PageContext, temp: Option<f64>) -> FanLine {
    let numbers = context.numbers;
    let name = overview::name(&card.row.name, numbers);
    let has_mode = matches!(card.kind, CardKind::Full { .. });
    let mode = card_mode(card).filter(|_| has_mode);
    let (chart, now, chart_description) = match card.chart() {
        Some(data) => {
            let points = data
                .points
                .iter()
                .map(|p| (f64::from(p.temp), f64::from(p.duty)))
                .collect();
            let reading = data
                .millidegrees
                .zip(card.duty)
                .map(|(millidegrees, duty)| Reading { millidegrees, duty });
            let now = reading.map(|r| (r.millidegrees as f64 / 1000.0, f64::from(r.duty)));
            let source = data
                .own_source
                .as_ref()
                .or(context.source)
                .map(|s| source_name(s, numbers))
                .unwrap_or_default();
            let curve = ChartCurve {
                name: name.clone(),
                source,
                points: data.points.clone(),
                min_percent: card.min_percent,
                threshold: data.threshold,
                needs_full: data.needs_full,
            };
            (
                points,
                now,
                Some(fan_chart::describe(&curve, reading, numbers)),
            )
        }
        // Manual mode: the flat line of the duty cycle set.
        None if mode == Some(Mode::Manual) => match card.duty.map(f64::from) {
            Some(duty) => (
                vec![(0.0, duty), (100.0, duty)],
                temp.map(|t| (t, duty)),
                None,
            ),
            None => (Vec::new(), None, None),
        },
        None => (Vec::new(), None, None),
    };
    FanLine {
        key: card.key.clone(),
        name,
        header: card.header.clone(),
        rpm: card.row.value,
        duty: card.duty,
        has_mode,
        mode,
        min_percent: card.min_percent,
        target: card.moving_to(),
        unchecked: !card.verified,
        // The extra header is read only with every driver; as on the
        // Overview, its row says nothing about an empty header.
        maybe_empty: card.maybe_empty() && card.role != FanRole::Aux,
        opens: card.chart().is_some(),
        chart,
        now,
        chart_description,
    }
}

/// A row of the list for a header the driver does not control (JAF_1), as
/// the Overview shows it.
fn read_only_line(row: &Row, numbers: &Numbers) -> FanLine {
    let (header, duty, verified) = match &row.subtitle {
        Subtitle::Header {
            header,
            duty,
            verified,
            ..
        } => (
            header.clone(),
            duty.filter(|d| d.is_finite())
                .map(|d| d.round().clamp(0.0, 255.0) as u8),
            *verified,
        ),
        _ => (String::new(), None, true),
    };
    FanLine {
        key: row.id.channel.clone(),
        name: overview::name(&row.name, numbers),
        header,
        rpm: row.value,
        duty,
        has_mode: false,
        mode: None,
        min_percent: 0,
        target: None,
        unchecked: !verified,
        maybe_empty: false,
        chart: Vec::new(),
        now: None,
        chart_description: None,
        opens: false,
    }
}

fn set_label(label: &gtk::Label, text: &str) {
    if label.label() != text {
        label.set_label(text);
    }
}

#[derive(Default)]
struct State {
    /// What decides whether the rows are made again: key, whether the row
    /// opens the curve, whether it has a chart.
    shapes: Vec<(String, bool, bool)>,
    rows: Vec<FanRow>,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/fans.ui")]
    pub struct FansPage {
        #[template_child]
        pub bin: TemplateChild<adw::BreakpointBin>,
        #[template_child]
        pub one_column: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub narrow: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub details: TemplateChild<gtk::Button>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub hero: TemplateChild<gtk::Box>,
        #[template_child]
        pub hero_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub hero_value: TemplateChild<gtk::Label>,
        #[template_child]
        pub hero_unit: TemplateChild<gtk::Label>,
        #[template_child]
        pub speeding: TemplateChild<gtk::Label>,
        #[template_child]
        pub kernel: TemplateChild<gtk::Label>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub footer: TemplateChild<gtk::Label>,
        /// The board profile; `None` in generic mode.
        pub(super) profile: RefCell<Option<Profile>>,
        /// Why there is no profile, as the System page says it.
        pub(super) no_profile: RefCell<String>,
        pub(super) visibility: RefCell<Visibility>,
        pub(super) numbers: OnceCell<Numbers>,
        pub(super) layout: Cell<Layout>,
        pub(super) state: RefCell<State>,
        /// The latest sample; the page shows it when it is mapped.
        pub(super) last: RefCell<Option<Sample>>,
        /// The model shown.
        pub(super) model: RefCell<Option<Page>>,
        /// The curve page, while it is open.
        pub(super) curve: RefCell<Option<CurvePage>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FansPage {
        const NAME: &'static str = "BaseboardFansPage";
        type Type = super::FansPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FansPage {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.bin.connect_current_breakpoint_notify(glib::clone!(
                #[weak]
                obj,
                move |_| obj.update_layout()
            ));
            // Samples are only stored while the page cannot be seen.
            obj.connect_map(|page| page.display());

            self.speeding
                .update_state(&[gtk::accessible::State::Hidden(true)]);
            self.kernel.set_label(&kernel_markup());
            self.kernel.connect_activate_link(|label, uri| {
                let window = label.root().and_downcast::<gtk::Window>();
                gtk::UriLauncher::new(uri).launch(
                    window.as_ref(),
                    gio::Cancellable::NONE,
                    |result| {
                        if let Err(e) = result {
                            eprintln!("baseboard: cannot open the website: {e}");
                        }
                    },
                );
                glib::Propagation::Stop
            });
            self.footer.set_label(&footer_text());
            // Translators: read by screen readers for the list of fans.
            let name = gettext("Fans");
            self.list
                .update_property(&[gtk::accessible::Property::Label(&name)]);
        }
    }

    impl WidgetImpl for FansPage {}
    impl BinImpl for FansPage {}
}

glib::wrapper! {
    pub struct FansPage(ObjectSubclass<imp::FansPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FansPage {
    /// The board: its profile, or why there is none (generic mode).
    pub fn set_board(&self, profile: Option<Profile>, no_profile: String) {
        let imp = self.imp();
        imp.profile.replace(profile);
        imp.no_profile.replace(no_profile);
        self.display();
    }

    /// The readings turned off in the preferences; their rows go.
    pub fn set_visibility(&self, visibility: Visibility) {
        self.imp().visibility.replace(visibility);
        self.display();
    }

    /// A new sample from the window's sensor reader.
    pub fn show(&self, sample: &Sample) {
        let imp = self.imp();
        imp.last.replace(Some(sample.clone()));
        // Under an open curve page this page is not mapped, but the curve
        // page shows its values.
        if self.is_mapped() || imp.curve.borrow().is_some() {
            self.display();
        }
    }

    /// The curve page of header `key`, which the window pushes; `None`
    /// when the header has no curve now.
    pub fn curve_page(&self, key: &str) -> Option<adw::NavigationPage> {
        let imp = self.imp();
        let model = imp.model.borrow();
        let model = model.as_ref()?;
        let card = model
            .cards
            .iter()
            .find(|c| c.key == key && c.chart().is_some())?;
        let curve = CurvePage::new(key);
        curve.set_layout(imp.layout.get());
        let numbers = self.numbers();
        curve.update(
            card,
            &PageContext {
                numbers,
                source: model.source.as_ref(),
            },
            &preview_note(),
        );
        let page = curve.page.clone();
        // Gone back: the page is not updated any more.
        page.connect_hidden(glib::clone!(
            #[weak(rename_to = fans)]
            self,
            move |page| {
                let imp = fans.imp();
                let open = imp.curve.borrow().as_ref().is_some_and(|c| c.page == *page);
                if open {
                    imp.curve.replace(None);
                }
            }
        ));
        imp.curve.replace(Some(curve));
        Some(page)
    }

    /// Scrolls to the end of the page (debug screenshots).
    #[cfg(debug_assertions)]
    pub fn scroll_to_end(&self) {
        let adjustment = self.imp().scrolled.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }

    fn numbers(&self) -> &Numbers {
        self.imp()
            .numbers
            .get_or_init(|| Numbers::new(&crate::i18n::decimal_separator()))
    }

    fn update_layout(&self) {
        let imp = self.imp();
        let current = imp.bin.current_breakpoint();
        let layout = if current.as_ref() == Some(&*imp.narrow) {
            Layout::Narrow
        } else if current.as_ref() == Some(&*imp.one_column) {
            Layout::OneColumn
        } else {
            Layout::Wide
        };
        if imp.layout.replace(layout) == layout {
            return;
        }
        self.apply_layout();
    }

    fn apply_layout(&self) {
        let imp = self.imp();
        let layout = imp.layout.get();
        let narrow = layout == Layout::Narrow;
        if narrow {
            imp.hero.add_css_class("narrow");
        } else {
            imp.hero.remove_css_class("narrow");
        }
        let numbers = self.numbers();
        for row in &imp.state.borrow().rows {
            row.set_narrow(narrow, numbers);
        }
        if let Some(curve) = imp.curve.borrow().as_ref() {
            curve.set_layout(layout);
        }
    }

    /// A status page in place of the list.
    fn show_status(&self, title: &str, description: Option<&str>, details: bool) {
        let imp = self.imp();
        imp.status.set_title(title);
        imp.status.set_description(description);
        imp.details.set_visible(details);
        imp.stack.set_visible_child_name("status");
        imp.model.replace(None);
        self.close_curve();
    }

    /// Goes back from the curve page, e.g. when its header is gone.
    fn close_curve(&self) {
        let Some(curve) = self.imp().curve.take() else {
            return;
        };
        if let Some(view) = curve
            .page
            .ancestor(adw::NavigationView::static_type())
            .and_downcast::<adw::NavigationView>()
        {
            view.pop_to_tag("root");
        }
    }

    fn display(&self) {
        let imp = self.imp();
        let profile = imp.profile.borrow();
        let unavailable = gettext("Fan control unavailable");
        let Some(profile) = profile.as_ref() else {
            let reason = imp.no_profile.borrow().clone();
            self.show_status(&unavailable, Some(&reason), true);
            return;
        };
        let last = imp.last.borrow();
        let Some(sample) = last.as_ref() else {
            return;
        };
        if let Some(chip) = &sample.status.missing_chip {
            // Translators: {} is the name of the board's chip, e.g. nct6687.
            let reason = gettext("The board's {} chip was not found").replace("{}", chip);
            self.show_status(&unavailable, Some(&reason), true);
            return;
        }
        // The fan channels come with the next sample after entering.
        let Some(channels) = &sample.fans else {
            return;
        };
        let model = page::build(&page::Input {
            profile,
            groups: &sample.groups,
            channels,
            visibility: &imp.visibility.borrow(),
        });
        if model.cards.is_empty() && model.read_only.is_empty() {
            self.show_status(
                &gettext("All readings are hidden"),
                Some(&gettext("Turn them on again in Preferences.")),
                false,
            );
            return;
        }
        self.render(&model);
        if imp.stack.visible_child_name().as_deref() != Some("fans") {
            imp.stack.set_visible_child_name("fans");
        }
        imp.model.replace(Some(model));
    }

    fn render(&self, model: &Page) {
        let imp = self.imp();
        let numbers = self.numbers();
        let context = PageContext {
            numbers,
            source: model.source.as_ref(),
        };

        // The temperature on top, in whole degrees.
        let temperature = model.temperature.as_ref();
        let value = temperature
            .and_then(|t| t.row.as_ref())
            .map(|r| r.value)
            .filter(|v| v.is_finite());
        for widget in [
            imp.hero_label.upcast_ref::<gtk::Widget>(),
            imp.hero_value.upcast_ref(),
            imp.hero_unit.upcast_ref(),
        ] {
            widget.set_visible(value.is_some());
        }
        if let (Some(temperature), Some(value)) = (temperature, value) {
            set_label(&imp.hero_label, &temperature_name(temperature, numbers));
            set_label(&imp.hero_value, &format!("{}", value.round() as i64));
        }

        // The rows.
        let mut lines: Vec<FanLine> = model
            .cards
            .iter()
            .map(|card| card_line(card, &context, value))
            .collect();
        lines.extend(model.read_only.iter().map(|r| read_only_line(r, numbers)));

        // Under the temperature: the reserved line, or what the kernel can
        // do.
        let no_control = matches!(model.note, Note::NoControl { .. });
        imp.kernel.set_visible(no_control);
        imp.speeding.set_visible(!no_control);
        imp.footer.set_visible(!no_control);
        let speeding = !no_control && row::speeding_up(&lines);
        let opacity = if speeding { 1.0 } else { 0.0 };
        if imp.speeding.opacity() != opacity {
            imp.speeding.set_opacity(opacity);
            imp.speeding
                .update_state(&[gtk::accessible::State::Hidden(!speeding)]);
        }

        let narrow = imp.layout.get() == Layout::Narrow;
        let shapes: Vec<(String, bool, bool)> = lines
            .iter()
            .map(|line| (line.key.clone(), line.opens, line.chart.is_empty()))
            .collect();
        let mut state = imp.state.borrow_mut();
        if state.shapes != shapes {
            imp.list.remove_all();
            state.rows = lines
                .iter()
                .map(|line| FanRow::new(line, numbers, narrow))
                .collect();
            for row in &state.rows {
                imp.list.append(&row.row);
            }
            state.shapes = shapes;
        } else {
            for (row, line) in state.rows.iter().zip(&lines) {
                row.update(line, numbers);
            }
        }
        drop(state);

        // The curve page, while it is open; it goes back when its header
        // has no curve any more.
        let gone = match imp.curve.borrow().as_ref() {
            Some(curve) => match model
                .cards
                .iter()
                .find(|c| c.key == curve.key && c.chart().is_some())
            {
                Some(card) => {
                    curve.update(card, &context, &preview_note());
                    false
                }
                None => true,
            },
            None => false,
        };
        if gone {
            self.close_curve();
        }
    }
}

#[cfg(test)]
mod tests {
    use baseboard_core::profile::{Temp, TempRole};

    use super::*;

    fn source(source: Source, temp: Option<Temp>) -> SourceRow {
        SourceRow {
            source,
            temp,
            row: None,
            chip: "nct6687".into(),
        }
    }

    fn cpu() -> Temp {
        Temp {
            key: "cpu".into(),
            role: TempRole::Cpu,
            input: 1,
            emergency_c: Some(95),
            label: None,
        }
    }

    #[test]
    fn names_of_sources() {
        let numbers = Numbers::new(".");
        let name = |s: SourceRow| source_name(&s, &numbers);
        assert_eq!(
            name(source(Source::Temp(1), Some(cpu()))),
            "CPU (board sensor)"
        );
        // A temperature the profile does not name, by its channel.
        assert_eq!(name(source(Source::Temp(5), None)), "temp5");
        assert_eq!(
            name(source(Source::Mask(0b101), None)),
            "Several temperatures: temp1, temp3"
        );
        assert_eq!(name(source(Source::Mask(0), None)), "No temperature");
        assert_eq!(
            name(source(Source::Unreported, None)),
            "A temperature the driver does not report"
        );
        assert_eq!(percent(30), "30\u{a0}%");
    }

    /// On top, the processor is just "CPU"; another source by its name.
    #[test]
    fn name_of_the_temperature_on_top() {
        let numbers = Numbers::new(".");
        assert_eq!(
            temperature_name(&source(Source::Temp(1), Some(cpu())), &numbers),
            "CPU"
        );
        assert_eq!(
            temperature_name(&source(Source::Temp(5), None), &numbers),
            "temp5"
        );
    }

    /// A wrapped text never leaves fewer than three words alone on its last
    /// line (D60).
    #[test]
    fn last_words_held_together() {
        assert_eq!(keep_last_words("a b c d e", 3), "a b c\u{a0}d\u{a0}e");
        assert_eq!(keep_last_words("one two", 3), "one\u{a0}two");
        assert_eq!(keep_last_words("one", 3), "one");
        assert_eq!(
            footer_text(),
            "For now, Baseboard only shows the fan settings from\u{a0}the\u{a0}BIOS."
        );
    }

    /// The kernel section of the website; the Polish translation names the
    /// Polish page. The link is a real link.
    #[test]
    fn kernel_section_of_the_website() {
        assert_eq!(kernel_url(), "https://baseboard.bojanowski.me/#kernel");
        assert!(
            kernel_markup().ends_with(
                " <a href=\"https://baseboard.bojanowski.me/#kernel\">Kernel status</a>"
            ),
            "{}",
            kernel_markup()
        );
    }

    /// Four marks in the legend, each with a name.
    #[test]
    fn legend_of_the_charts() {
        let items = legend_items();
        let marks: Vec<Mark> = items.iter().map(|(mark, _)| *mark).collect();
        assert_eq!(
            marks,
            [Mark::BelowMinimum, Mark::Threshold, Mark::Now, Mark::Ends]
        );
        assert!(items.iter().all(|(_, text)| !text.is_empty()));
    }
}
