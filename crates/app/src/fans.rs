// SPDX-License-Identifier: GPL-3.0-or-later

//! The Fans page (docs/PROJEKT.md §5.5.3, §5.5.4, §7 M2, M2.4, D62, D67)
//! of version 0.2.0: what the board's controller does with the fans,
//! without changing anything, in plain words. The processor's card, the
//! same as on the Overview, beside a list of the fans as wide as in GNOME
//! Settings at most, a row each with a small chart; in a narrow window the
//! card stands above the list. The model comes from `core::fans::page`;
//! this module gives it texts and widgets. Rows are made again only when
//! the fans change (another driver after a reload, a header turned off),
//! otherwise their values are set. Samples come from the window's sensor
//! reader, which reads the fan channels only while this page is shown.

mod curve_page;
mod row;
mod spark;

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::driver::DriverLevel;
use baseboard_core::fans::driver::{Mode, Source};
use baseboard_core::fans::page::{self, Fan, FanKind, Page, SourceRow};
use baseboard_core::history::Histories;
use baseboard_core::overview::{Name, Row, RowId, Subtitle, Visibility};
use baseboard_core::profile::{FanRole, Profile};
use baseboard_core::summary::{self, HeroKind};
use baseboard_core::units::{Numbers, UNIT_SPACE};
use gettextrs::gettext;
use gtk::{gdk, gio, glib, pango};

use crate::alert::{self, Alert};
use crate::config;
use crate::fan_chart::swatch::Mark;
use crate::fan_chart::{self, ChartCurve, Reading};
use crate::layout::{self, Width};
use crate::overview;
use crate::part_card::PartCard;
use crate::sampler::Sample;
use curve_page::{CurvePage, fan_mode};
use row::{FanLine, FanRow};

/// What a row and the curve page need besides their fan.
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

/// The icon after the link "Kernel status" and the gap before it, in
/// pixels, as in the mockup.
const LINK_ICON: i32 = 14;
const LINK_GAP: i32 = 4;

/// Under the processor's card on a driver without control (D62): what the
/// system can do, with a link to the kernel section of the website. The
/// link is bold and never breaks; it ends in a no-break space that keeps
/// the room for its icon (`reserve_link_icon`).
fn kernel_markup() -> String {
    let text = crate::system::read_only_fans();
    let link = gettext(
        // Translators: a link after the sentence about the kernel on the
        // Fans page; it opens the website's section on fan control in the
        // kernel.
        "Kernel status",
    );
    format!(
        "{} <a href=\"{}\"><b>{}</b>\u{a0}</a>",
        glib::markup_escape_text(&text),
        glib::markup_escape_text(&kernel_url()),
        glib::markup_escape_text(&link.replace(' ', "\u{a0}"))
    )
}

/// The byte index of the last character of `label`: the no-break space
/// at the end of the link, where its icon stands.
fn link_icon_index(label: &gtk::Label) -> Option<usize> {
    label.text().len().checked_sub('\u{a0}'.len_utf8())
}

/// Leaves room for the icon of the link: the space at its end gets the
/// width of the gap and the icon. A Pango shape draws nothing; the icon is
/// a widget over the label (`link_icon_place`).
fn reserve_link_icon(label: &gtk::Label) {
    let Some(index) = link_icon_index(label).and_then(|i| u32::try_from(i).ok()) else {
        return;
    };
    let room = pango::Rectangle::new(0, 0, (LINK_GAP + LINK_ICON) * pango::SCALE, 0);
    let mut shape = pango::AttrShape::new(&room, &room);
    shape.set_start_index(index);
    shape.set_end_index(index + '\u{a0}'.len_utf8() as u32);
    let attributes = pango::AttrList::new();
    attributes.insert(shape);
    label.set_attributes(Some(&attributes));
}

/// Where the icon of the link stands in the label: after the gap in the
/// room at the end of the link, in the middle of its line. The room has no
/// height of its own, so the height is the line's.
fn link_icon_place(label: &gtk::Label) -> Option<gdk::Rectangle> {
    let index = i32::try_from(link_icon_index(label)?).ok()?;
    let layout = label.layout();
    let (line, _) = layout.index_to_line_x(index, false);
    let mut lines = layout.iter();
    for _ in 0..line {
        lines.next_line();
    }
    let (_, extents) = lines.line_extents();
    let (x, y) = label.layout_offsets();
    let height = extents.height() / pango::SCALE;
    Some(gdk::Rectangle::new(
        x + layout.index_to_pos(index).x() / pango::SCALE + LINK_GAP,
        y + extents.y() / pango::SCALE + (height - LINK_ICON) / 2,
        LINK_ICON,
        LINK_ICON,
    ))
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

/// A row of the list for a fan of the model. `temp` is the temperature on
/// top, in °C, for the point of a fan in manual mode; `stopped` are the
/// CPU fans that stopped spinning (D65).
fn fan_line(fan: &Fan, context: &PageContext, temp: Option<f64>, stopped: &[RowId]) -> FanLine {
    let numbers = context.numbers;
    let name = overview::name(&fan.row.name, numbers);
    let has_mode = matches!(fan.kind, FanKind::Full { .. });
    let mode = fan_mode(fan).filter(|_| has_mode);
    let (chart, now, chart_description) = match fan.chart() {
        Some(data) => {
            let points = data
                .points
                .iter()
                .map(|p| (f64::from(p.temp), f64::from(p.duty)))
                .collect();
            let reading = data
                .millidegrees
                .zip(fan.duty)
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
                min_percent: fan.min_percent,
                threshold: data.threshold,
                needs_full: data.needs_full,
                in_effect: true,
            };
            (
                points,
                now,
                Some(fan_chart::describe(&curve, reading, numbers)),
            )
        }
        // Manual mode: the flat line of the duty cycle set.
        None if mode == Some(Mode::Manual) => match fan.duty.map(f64::from) {
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
        key: fan.key.clone(),
        name,
        header: fan.header.clone(),
        rpm: fan.row.value,
        duty: fan.duty,
        has_mode,
        mode,
        min_percent: fan.min_percent,
        target: fan.moving_to(),
        verified: fan.verified,
        // The extra header is read only with every driver; as on the
        // Overview, its row says nothing about an empty header.
        maybe_empty: fan.maybe_empty() && fan.role != FanRole::Aux,
        stopped: stopped.contains(&fan.row.id),
        curve_unreadable: fan.curve_unreadable(),
        // Also in manual mode: the curve stays in the controller.
        opens: fan.has_details(),
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
        verified,
        maybe_empty: false,
        stopped: false,
        curve_unreadable: false,
        chart: Vec::new(),
        now: None,
        chart_description: None,
        opens: false,
    }
}

/// The rows of the list are narrow in a narrow window, and beside the
/// card in a window of about 1200 px, where their column is.
fn narrow_rows(width: Width) -> bool {
    matches!(width, Width::Narrow | Width::Compact)
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
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub details: TemplateChild<gtk::Button>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub content: TemplateChild<gtk::Box>,
        #[template_child]
        pub columns: TemplateChild<gtk::Grid>,
        #[template_child]
        pub side: TemplateChild<gtk::Box>,
        #[template_child]
        pub kernel_box: TemplateChild<gtk::Overlay>,
        #[template_child]
        pub kernel: TemplateChild<gtk::Label>,
        #[template_child]
        pub list_clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub footer: TemplateChild<gtk::Label>,
        /// The processor's card, the same as on the Overview (D67).
        pub(super) card: OnceCell<PartCard>,
        /// The last five minutes of the temperatures, which the window
        /// keeps.
        pub(super) histories: OnceCell<Rc<RefCell<Histories>>>,
        /// The warning about a stopped CPU fan (D65).
        pub(super) alert: OnceCell<Alert>,
        /// The board profile; `None` without one.
        pub(super) profile: RefCell<Option<Profile>>,
        pub(super) visibility: RefCell<Visibility>,
        pub(super) numbers: OnceCell<Numbers>,
        pub(super) width: Cell<Option<Width>>,
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
            // Samples are only stored while the page cannot be seen.
            obj.connect_map(|page| page.display());

            let card = PartCard::new(
                "baseboard-processor-symbolic",
                &overview::hero_name(HeroKind::Cpu),
                None,
            );
            // Always in place, so that the list does not move when it shows
            // (D62).
            card.reserve_note(&gettext("Fans are speeding up"));
            self.side.prepend(&card.widget);
            let _ = self.card.set(card);
            layout::limit(&self.clamp);
            let page = obj.downgrade();
            layout::watch(&self.bin, move |width| {
                if let Some(page) = page.upgrade() {
                    page.set_width(width);
                }
            });

            self.kernel.set_label(&kernel_markup());
            reserve_link_icon(&self.kernel);
            // The label is laid out before the icon over it.
            self.kernel_box.connect_get_child_position(glib::clone!(
                #[weak(rename_to = label)]
                self.kernel,
                #[upgrade_or]
                None,
                move |_, _| link_icon_place(&label)
            ));
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
            let alert = Alert::new();
            self.content.prepend(&alert.widget);
            let _ = self.alert.set(alert);
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
    /// The board's profile, if there is one.
    pub fn set_board(&self, profile: Option<Profile>) {
        self.imp().profile.replace(profile);
        self.display();
    }

    /// The readings turned off in the preferences; their rows go.
    pub fn set_visibility(&self, visibility: Visibility) {
        self.imp().visibility.replace(visibility);
        self.display();
    }

    /// The histories of the temperatures, which the window keeps.
    pub fn set_histories(&self, histories: Rc<RefCell<Histories>>) {
        let _ = self.imp().histories.set(histories);
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
    /// when the header has no curve details to show.
    pub fn curve_page(&self, key: &str) -> Option<adw::NavigationPage> {
        let imp = self.imp();
        let model = imp.model.borrow();
        let model = model.as_ref()?;
        let fan = model
            .fans
            .iter()
            .find(|f| f.key == key && f.has_details())?;
        let curve = CurvePage::new(key);
        curve.set_layout(imp.width.get().unwrap_or_default());
        let numbers = self.numbers();
        curve.update(
            fan,
            &PageContext {
                numbers,
                source: model.source.as_ref(),
            },
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

    /// The card beside the list when there is room for two columns (D67),
    /// above it otherwise; the rows narrow when their column is.
    fn set_width(&self, width: Width) {
        let imp = self.imp();
        if imp.width.replace(Some(width)) == Some(width) {
            return;
        }
        for part in [
            imp.side.upcast_ref::<gtk::Widget>(),
            imp.list_clamp.upcast_ref(),
        ] {
            if part.parent().is_some() {
                imp.columns.remove(part);
            }
        }
        if width.columns() {
            imp.columns.attach(&*imp.side, 0, 0, 1, 1);
            imp.columns.attach(&*imp.list_clamp, 1, 0, 1, 1);
        } else {
            imp.columns.attach(&*imp.side, 0, 0, 1, 1);
            imp.columns.attach(&*imp.list_clamp, 0, 1, 1, 1);
        }
        if let Some(card) = imp.card.get() {
            card.set_width(width);
        }
        let numbers = self.numbers();
        for row in &imp.state.borrow().rows {
            row.set_narrow(narrow_rows(width), numbers);
        }
        if let Some(curve) = imp.curve.borrow().as_ref() {
            curve.set_layout(width);
        }
    }

    /// The processor's card as on the Overview (D67), with "Fans are
    /// speeding up" under it while one does.
    fn show_card(&self, sample: &Sample, speeding: bool) {
        let imp = self.imp();
        let (Some(card), Some(histories)) = (imp.card.get(), imp.histories.get()) else {
            return;
        };
        let groups = baseboard_core::overview::visible(&sample.groups, &imp.visibility.borrow());
        let top = summary::summary(&groups, &sample.stopped, sample.speeding);
        let state = summary::state(&top, &sample.groups, &sample.hot, true);
        let hero = top.heroes.iter().find(|h| h.kind == HeroKind::Cpu);
        card.widget.set_visible(hero.is_some());
        if let Some(hero) = hero {
            let mut view =
                overview::card_view(hero, &top, &state, &histories.borrow(), self.numbers());
            view.note = speeding.then(|| gettext("Fans are speeding up"));
            card.update(&view);
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
            self.show_status(&unavailable, Some(&crate::system::no_profile()), true);
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
        if model.fans.is_empty() && model.read_only.is_empty() {
            self.show_status(
                &gettext("All readings are hidden"),
                Some(&gettext("Turn them on again in Preferences.")),
                false,
            );
            return;
        }
        self.render(&model, sample);
        if imp.stack.visible_child_name().as_deref() != Some("fans") {
            imp.stack.set_visible_child_name("fans");
        }
        imp.model.replace(Some(model));
    }

    /// The page for `model` of `sample`.
    fn render(&self, model: &Page, sample: &Sample) {
        let imp = self.imp();
        let numbers = self.numbers();
        let context = PageContext {
            numbers,
            source: model.source.as_ref(),
        };
        let stopped = &sample.stopped;

        // A stopped CPU fan: the warning on top, as on the Overview and the
        // System page.
        if let Some(alert) = imp.alert.get() {
            let shown = model.fans.iter().any(|f| stopped.contains(&f.row.id));
            let (title, why) = (alert::cpu_fan_stopped(), alert::cpu_fan_stopped_why());
            alert.set(shown.then_some((title.as_str(), why.as_str())));
        }

        // The rows; a fan in manual mode has its point at the temperature
        // the curves follow.
        let value = model
            .temperature
            .as_ref()
            .and_then(|t| t.row.as_ref())
            .map(|r| r.value)
            .filter(|v| v.is_finite());
        let mut lines: Vec<FanLine> = model
            .fans
            .iter()
            .map(|fan| fan_line(fan, &context, value, stopped))
            .collect();
        lines.extend(model.read_only.iter().map(|r| read_only_line(r, numbers)));

        // The card with the reserved line, and under it what the kernel can
        // do.
        let no_control = model.level == DriverLevel::ReadOnly;
        imp.kernel_box.set_visible(no_control);
        imp.footer.set_visible(!no_control);
        let speeding = !no_control && row::speeding_up(&lines);
        self.show_card(sample, speeding);

        let narrow = narrow_rows(imp.width.get().unwrap_or_default());
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
        // has no curve details any more.
        let gone = match imp.curve.borrow().as_ref() {
            Some(curve) => match model
                .fans
                .iter()
                .find(|f| f.key == curve.key && f.has_details())
            {
                Some(fan) => {
                    curve.update(fan, &context);
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
            "Processor (board sensor)"
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

    /// In a wide window the list stands beside the card as wide as in GNOME
    /// Settings, 600 px, so that the sentences of its rows fit (D62, D67);
    /// in a window of about 1200 px it takes its column. Needs a display.
    #[test]
    fn list_beside_the_card() {
        crate::gtk_test::run("list_beside_the_card", || {
            crate::gtk_test::resources();
            let page = glib::Object::new::<FansPage>();
            let imp = page.imp();
            imp.stack.set_visible_child_name("fans");
            let list_at = |width: i32| {
                let (_, height, _, _) = page.measure(gtk::Orientation::Vertical, width);
                page.allocate(width, height, -1, None);
                imp.list_clamp.child().map_or(0, |list| list.width())
            };
            // 1850 px and 1200 px windows without their sidebars.
            assert_eq!(list_at(1570), 600);
            assert_eq!(imp.columns.query_child(&*imp.list_clamp).0, 1);
            let compact = list_at(920);
            assert!((400..=440).contains(&compact), "{compact}");
        });
    }

    /// The icon of the link stands right after the link text, on its
    /// line, in a wide and in a narrow label. Needs a display.
    #[test]
    fn icon_of_the_kernel_link() {
        crate::gtk_test::run("icon_of_the_kernel_link", || {
            let label = gtk::Label::builder()
                .use_markup(true)
                .wrap(true)
                .justify(gtk::Justification::Center)
                .label(kernel_markup())
                .build();
            reserve_link_icon(&label);
            // The sentence of the System page, then the link.
            assert!(label.text().starts_with(&crate::system::read_only_fans()));
            assert!(label.text().ends_with("Kernel\u{a0}status\u{a0}"));
            for width in [560, 200] {
                let (_, natural, _, _) = label.measure(gtk::Orientation::Vertical, width);
                label.allocate(width, natural, -1, None);
                let place = link_icon_place(&label).unwrap();
                let layout = label.layout();
                let (x, y) = label.layout_offsets();
                // The end of "status": the icon follows it after the gap.
                let text = label.text();
                let end = text.len() - '\u{a0}'.len_utf8();
                let status = layout.index_to_pos(i32::try_from(end).unwrap());
                assert_eq!(place.x(), x + status.x() / pango::SCALE + LINK_GAP);
                // The link stays whole on the last line, the icon with it.
                let last = layout.line_count() - 1;
                let (line, _) = layout.index_to_line_x(i32::try_from(end).unwrap(), false);
                assert_eq!(line, last, "width {width}");
                let start = text.find("Kernel").unwrap();
                let (first_line, _) = layout.index_to_line_x(i32::try_from(start).unwrap(), false);
                assert_eq!(first_line, last, "width {width}");
                // In the middle of the text of its line, not on the
                // baseline: its middle above the baseline, its top below
                // the top of the line.
                let mut lines = layout.iter();
                for _ in 0..last {
                    lines.next_line();
                }
                let (_, extents) = lines.line_extents();
                let baseline = y + lines.baseline() / pango::SCALE;
                let top = y + extents.y() / pango::SCALE;
                let middle = place.y() + LINK_ICON / 2;
                assert!(middle < baseline - 2, "width {width}");
                assert!(place.y() >= top, "width {width}");
                assert_eq!(place.width(), LINK_ICON);
                assert!(place.x() + LINK_ICON <= width, "width {width}");
            }
        });
    }
}
