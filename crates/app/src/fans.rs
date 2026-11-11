// SPDX-License-Identifier: GPL-3.0-or-later

//! The Fans page (docs/PROJEKT.md §5.5.3, §5.5.4, §7 M2, M2.4) of version
//! 0.2.0: what the board's controller does with the fans, without changing
//! anything. The model comes from `core::fans::page`; this module gives it
//! texts and widgets. Widgets are made again only when the cards change
//! (another driver after a reload, a header turned off), otherwise their
//! values are set. Samples come from the window's sensor reader, which
//! reads the fan channels only while this page is shown.

mod card;
mod curve_page;

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::driver::Source;
use baseboard_core::fans::page::{self, Note, Page, SourceRow};
use baseboard_core::overview::{Name, RowId, Visibility};
use baseboard_core::profile::Profile;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{Numbers, UNIT_SPACE};
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::config;
use crate::fan_chart::swatch::{Mark, Swatch};
use crate::overview::{self, RowWidgets};
use crate::sampler::Sample;
use card::{CardWidgets, Shape};
use curve_page::CurvePage;

/// How wide the page is (fans.ui): two columns of cards, one, or one with
/// the speed under the name.
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

/// The line on top of the page in version 0.2.0 at a driver that controls
/// the fans, and the note of the curve page.
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

/// The description of the temperature source: what the controller does
/// with it, and what the gray range is (D60, the same words as on the
/// Overview).
fn source_description() -> String {
    let what = gettext(
        // Translators: under "Temperature source" on the Fans page; the next
        // sentence follows on the same line.
        "The board's controller computes the duty cycles of the curves from this temperature.",
    );
    let range = gettext("Gray: minimum and maximum since the app started.");
    keep_last_words(&format!("{what} {range}"), 3)
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

/// The legend of the charts on the cards (D60): each mark with its name.
/// A name does not wrap, so the legend breaks between whole items.
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
        (Mark::Ends, gettext("curve beyond its end points")),
    ]
}

/// The row of the temperature source: value above the range in a narrow
/// window, next to it otherwise.
struct SourceWidgets {
    row: adw::ActionRow,
    values: gtk::Box,
    range: gtk::Inscription,
    value: gtk::Inscription,
}

impl SourceWidgets {
    fn new() -> Self {
        let row = adw::ActionRow::builder().use_markup(false).build();
        let icon = gtk::Image::from_icon_name("baseboard-temperature-symbolic");
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        row.add_prefix(&icon);
        let values = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        let range = gtk::Inscription::builder()
            .min_chars(11)
            .nat_chars(12)
            .xalign(1.0)
            .css_classes(["dimmed", "numeric", "caption"])
            .build();
        let value = gtk::Inscription::builder()
            .min_chars(8)
            .nat_chars(9)
            .xalign(1.0)
            .css_classes(["numeric"])
            .build();
        values.append(&range);
        values.append(&value);
        row.add_suffix(&values);
        Self {
            row,
            values,
            range,
            value,
        }
    }

    fn set_layout(&self, layout: Layout) {
        let narrow = layout == Layout::Narrow;
        self.values.set_orientation(if narrow {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        self.values.set_spacing(if narrow { 2 } else { 6 });
        // The value first, the range under it.
        if narrow {
            self.values
                .reorder_child_after(&self.range, Some(&self.value));
        } else {
            self.values
                .reorder_child_after(&self.value, Some(&self.range));
        }
    }

    fn update(&self, source: &SourceRow, numbers: &Numbers) {
        let title = source_name(source, numbers);
        let channel = match source.source {
            Source::Temp(input) => Some(format!("temp{input}")),
            _ => None,
        };
        let threshold = source.temp.as_ref().and_then(|t| t.emergency_c);
        let subtitle = match (&channel, threshold) {
            (Some(channel), Some(threshold)) => {
                // Translators: under the temperature source of the fan
                // curves, e.g. "nct6687 · temp1 · emergency threshold 95 °C".
                gettext("{chip} · {channel} · emergency threshold {temperature}")
                    .replace("{chip}", &source.chip)
                    .replace("{channel}", channel)
                    .replace("{temperature}", &format!("{threshold}{UNIT_SPACE}°C"))
            }
            (Some(channel), None) => format!("{} · {channel}", source.chip),
            (None, _) => source.chip.clone(),
        };
        let (value, range) = match &source.row {
            Some(row) => (
                numbers.value(Kind::Temp, row.value),
                row.range
                    .map(|(lo, hi)| numbers.range_of(Kind::Temp, lo, hi, None))
                    .unwrap_or_default(),
            ),
            None => (String::new(), String::new()),
        };
        if self.row.title() != title {
            self.row.set_title(&title);
        }
        if self.row.subtitle().as_deref() != Some(subtitle.as_str()) {
            self.row.set_subtitle(&subtitle);
        }
        if self.value.text().as_deref() != Some(value.as_str()) {
            self.value.set_text(Some(&value));
        }
        if self.range.text().as_deref() != Some(range.as_str()) {
            self.range.set_text(Some(&range));
            self.range.set_visible(!range.is_empty());
        }
        let description = if range.is_empty() {
            gettext("{subtitle}. Now {value}")
        } else {
            gettext("{subtitle}. Now {value}, since start {range}")
        }
        .replace("{subtitle}", &subtitle)
        .replace("{value}", &value)
        .replace("{range}", &range);
        self.row
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Description(&description)]);
    }
}

#[derive(Default)]
struct State {
    shapes: Vec<Shape>,
    cards: Vec<CardWidgets>,
    source: Option<SourceWidgets>,
    read_only: Vec<(RowId, RowWidgets)>,
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
        pub note_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub kernel_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub source_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub legend: TemplateChild<adw::WrapBox>,
        #[template_child]
        pub cards: TemplateChild<gtk::Grid>,
        #[template_child]
        pub read_only_group: TemplateChild<adw::PreferencesGroup>,
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
        /// The status boxes of the cards in one row, equally high (D60).
        pub(super) size_groups: RefCell<Vec<gtk::SizeGroup>>,
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

            self.source_group
                .set_description(Some(&glib::markup_escape_text(&source_description())));
            for (mark, text) in legend_items() {
                let item = gtk::Box::builder().spacing(7).build();
                item.append(&Swatch::new(mark));
                item.append(
                    &gtk::Label::builder()
                        .label(text)
                        .css_classes(["caption", "fan-soft"])
                        .build(),
                );
                self.legend.append(&item);
            }
            // Translators: read by screen readers for the legend above the
            // fan cards.
            let name = gettext("Legend of the charts");
            self.legend
                .upcast_ref::<gtk::Widget>()
                .update_property(&[gtk::accessible::Property::Label(&name)]);
            self.kernel_button.connect_clicked(|button| {
                let window = button.root().and_downcast::<gtk::Window>();
                gtk::UriLauncher::new(&kernel_url()).launch(
                    window.as_ref(),
                    gio::Cancellable::NONE,
                    |result| {
                        if let Err(e) = result {
                            eprintln!("baseboard: cannot open the website: {e}");
                        }
                    },
                );
            });
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

    /// The readings turned off in the preferences; their cards go.
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

    /// Puts the cards in rows of two, each as high as its higher card, or
    /// in one column. In a row of two the status boxes (the mode, or the
    /// box of an unchecked header) are equally high, so that the charts and
    /// the footers line up and no card has a gap under its chart (D60).
    fn arrange(&self, cards: &[CardWidgets], layout: Layout) {
        let imp = self.imp();
        let grid = &imp.cards;
        while let Some(child) = grid.first_child() {
            grid.remove(&child);
        }
        for group in imp.size_groups.take() {
            for widget in group.widgets() {
                group.remove_widget(&widget);
            }
        }
        let columns = if layout == Layout::Wide { 2 } else { 1 };
        for (i, card) in cards.iter().enumerate() {
            let (row, column) = (i / columns, i % columns);
            grid.attach(&card.card, column as i32, row as i32, 1, 1);
        }
        // A last card alone keeps half the width.
        if columns == 2 && cards.len() % 2 == 1 {
            let filler = gtk::Box::new(gtk::Orientation::Vertical, 0);
            grid.attach(&filler, 1, (cards.len() / 2) as i32, 1, 1);
        }
        if columns == 2 {
            let groups = cards
                .chunks(2)
                .filter_map(|row| {
                    let boxes: Vec<&gtk::Widget> =
                        row.iter().filter_map(CardWidgets::status).collect();
                    (boxes.len() == 2).then(|| {
                        let group = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
                        for status in boxes {
                            group.add_widget(status);
                        }
                        group
                    })
                })
                .collect();
            imp.size_groups.replace(groups);
        }
    }

    fn apply_layout(&self) {
        let imp = self.imp();
        let layout = imp.layout.get();
        let state = imp.state.borrow();
        self.arrange(&state.cards, layout);
        for card in &state.cards {
            card.set_layout(layout);
        }
        if let Some(source) = &state.source {
            source.set_layout(layout);
        }
        if let Some(curve) = imp.curve.borrow().as_ref() {
            curve.set_layout(layout);
        }
    }

    /// A status page in place of the cards.
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
        let layout = imp.layout.get();

        // The line on top, with the information icon in both cases: a driver
        // without control is the usual state of released kernels (D60).
        let (title, subtitle, kernel) = match model.note {
            Note::NoControl { sys_without_duty } => (
                gettext("Fan control unavailable"),
                crate::system::no_control(sys_without_duty),
                true,
            ),
            Note::PreviewOnly => (preview_note(), String::new(), false),
        };
        if imp.note_row.title() != title {
            imp.note_row.set_title(&title);
        }
        if imp.note_row.subtitle().as_deref() != Some(subtitle.as_str()) {
            imp.note_row.set_subtitle(&subtitle);
        }
        imp.kernel_button.set_visible(kernel);
        imp.legend
            .set_visible(model.cards.iter().any(|card| card.chart().is_some()));

        let mut state = imp.state.borrow_mut();

        // The temperature source.
        imp.source_group.set_visible(model.source.is_some());
        if let Some(source) = &model.source {
            let widgets = state.source.get_or_insert_with(|| {
                let widgets = SourceWidgets::new();
                imp.source_group.add(&widgets.row);
                widgets.set_layout(layout);
                widgets
            });
            widgets.update(source, numbers);
        }

        // The cards, made again when they change.
        let shapes: Vec<Shape> = model.cards.iter().map(Shape::of).collect();
        if state.shapes != shapes {
            state.cards = model
                .cards
                .iter()
                .map(|card| CardWidgets::new(card, layout))
                .collect();
            state.shapes = shapes;
            self.arrange(&state.cards, layout);
        }
        let context = PageContext {
            numbers,
            source: model.source.as_ref(),
        };
        for (widgets, card) in state.cards.iter().zip(&model.cards) {
            widgets.update(card, &context);
        }

        // The headers without control, as on the Overview.
        imp.read_only_group.set_visible(!model.read_only.is_empty());
        let ids: Vec<RowId> = model.read_only.iter().map(|r| r.id.clone()).collect();
        let shown: Vec<RowId> = state.read_only.iter().map(|(id, _)| id.clone()).collect();
        if ids != shown {
            for (_, old) in state.read_only.drain(..) {
                imp.read_only_group.remove(&old.row);
            }
            for row in &model.read_only {
                let widgets = RowWidgets::new(false);
                let icon = gtk::Image::builder()
                    .icon_name("baseboard-fan-symbolic")
                    .css_classes(["dimmed"])
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build();
                widgets.row.add_prefix(&icon);
                imp.read_only_group.add(&widgets.row);
                state.read_only.push((row.id.clone(), widgets));
            }
            let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
            imp.read_only_group
                .set_description(Some(&glib::markup_escape_text(&ngettext(
                    "The kernel driver does not support control of this header.",
                    "The kernel driver does not support control of these headers.",
                    count,
                ))));
        }
        for ((_, widgets), row) in state.read_only.iter().zip(&model.read_only) {
            widgets.update(row, numbers);
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

    #[test]
    fn names_of_sources() {
        let numbers = Numbers::new(".");
        let cpu = Temp {
            key: "cpu".into(),
            role: TempRole::Cpu,
            input: 1,
            emergency_c: Some(95),
            label: None,
        };
        let name = |s: SourceRow| source_name(&s, &numbers);
        assert_eq!(
            name(source(Source::Temp(1), Some(cpu))),
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

    /// A wrapped description never leaves fewer than three words alone on
    /// its last line (D60).
    #[test]
    fn last_words_held_together() {
        assert_eq!(keep_last_words("a b c d e", 3), "a b c\u{a0}d\u{a0}e");
        assert_eq!(keep_last_words("one two", 3), "one\u{a0}two");
        assert_eq!(keep_last_words("one", 3), "one");
        let description = source_description();
        assert!(
            description.starts_with("The board's controller computes the duty cycles"),
            "{description}"
        );
        assert!(
            description.ends_with(" since the\u{a0}app\u{a0}started."),
            "{description}"
        );
    }

    /// The kernel section of the website; the Polish translation names the
    /// Polish page.
    #[test]
    fn kernel_section_of_the_website() {
        assert_eq!(kernel_url(), "https://baseboard.bojanowski.me/#kernel");
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
