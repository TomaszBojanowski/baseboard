// SPDX-License-Identifier: GPL-3.0-or-later

//! Debug builds only: `BASEBOARD_DEBUG_FAN_CHARTS=1` opens a window with
//! the fan curve charts instead of the main window, to look at the chart
//! before the fan page exists (step 3 of M2a). It only reads sysfs.
//!
//! - A card chart for every channel with a curve, read once from the
//!   driver (`BASEBOARD_SYSFS_ROOT` picks a test tree), with the live
//!   source temperature and duty cycle from the sensor reader of the
//!   Overview, every second while the window is visible.
//! - SYS_FAN1 at a fixed 46 °C and duty cycle 76 (30 %), while the curve
//!   says 98: the fan moving to its target.
//! - Large charts: SYS_FAN1 as a preview without handles, CPU_FAN1 with
//!   numbered handles.
//!
//! `BASEBOARD_DEBUG_FAN_CHARTS=cards` shows only the cards, `=large` only
//! the large charts, so that each fits a screenshot.
//! `BASEBOARD_DEBUG_HIDE_AFTER=N` hides the window after N seconds, to
//! measure the cost of a hidden window; the app keeps running.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use baseboard_core::fans::driver::{self, FanDriver, Source, SysfsDriver};
use baseboard_core::overview::{Group, Name, Subtitle};
use baseboard_core::profile::{Fan, FanRole, Profile};
use baseboard_core::sensors::{self, SensorSource, SysfsSource};
use baseboard_core::units::Numbers;
use gtk::glib;

use crate::config;
use crate::fan_chart::{ChartCurve, FanChart, Reading, Size};
use crate::overview;
use crate::sampler::{FanReads, Options, Setup, Worker};

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn wanted() -> bool {
    var("BASEBOARD_DEBUG_FAN_CHARTS").is_some()
}

/// A chart that follows one fan: its duty cycle (the `fanN` row) and its
/// source temperature (the `tempN` row of the board's chip).
struct Live {
    chart: FanChart,
    fan: String,
    temp: Option<String>,
}

thread_local! {
    /// The charts that follow the sensor reader, on the main thread.
    static LIVE: RefCell<Vec<Live>> = const { RefCell::new(Vec::new()) };
}

/// A channel with a curve.
struct Channel {
    fan: Fan,
    curve: ChartCurve,
    temp_input: Option<u8>,
}

fn channels(profile: &Profile, numbers: &Numbers) -> Vec<Channel> {
    let chips = SysfsSource::new(&config::sysfs_root()).chips();
    let Some(chip) = sensors::find(&chips, &profile.hwmon.name) else {
        eprintln!("baseboard: no hwmon device named {}", profile.hwmon.name);
        return Vec::new();
    };
    let driver = SysfsDriver::new(chip.path.clone());
    profile
        .fans
        .iter()
        .filter_map(|fan| {
            let pwm = fan.pwm_index?;
            let points = driver.curve(pwm).ok().filter(|p| !p.is_empty())?;
            let source = driver.source(pwm).ok();
            let temp = source.and_then(|s| driver::source_temp(profile, s));
            if let Some(Source::Mask(_) | Source::Unreported) = source {
                eprintln!("baseboard: {} follows no single temperature", fan.key);
            }
            let name = fan.label.clone().unwrap_or_else(|| {
                overview::name(
                    &Name::Fan {
                        role: fan.role,
                        number: fan.number,
                    },
                    numbers,
                )
            });
            Some(Channel {
                fan: fan.clone(),
                curve: ChartCurve {
                    name,
                    source: temp
                        .map(|t| overview::name(&Name::Temp(t.role), numbers))
                        .unwrap_or_default(),
                    points,
                    min_percent: fan.min_duty(),
                    threshold: temp.and_then(|t| t.emergency_c),
                    needs_full: matches!(fan.role, FanRole::Cpu | FanRole::Pump),
                    in_effect: true,
                },
                temp_input: temp.map(|t| t.input),
            })
        })
        .collect()
}

/// A card like the connector cards of the mockups: name, header, chart.
fn card(title: &str, header: &str, chart: &FanChart) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .css_classes(["card"])
        .build();
    let name = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["heading"])
        .margin_top(12)
        .margin_start(14)
        .margin_end(14)
        .build();
    let subtitle = gtk::Label::builder()
        .label(header)
        .xalign(0.0)
        .css_classes(["caption", "dimmed"])
        .margin_start(14)
        .margin_end(14)
        .margin_bottom(4)
        .build();
    // The charts as wide as in the mockups (411 and 868 px), with the
    // labels of 0 and 100 °C reaching into the margin.
    let margin = if chart.size() == Size::Card { 7 } else { 2 };
    chart.set_margin_start(margin);
    chart.set_margin_end(margin);
    chart.set_margin_bottom(14);
    card.append(&name);
    card.append(&subtitle);
    card.append(chart);
    card
}

fn live(chart: &FanChart, channel: &Channel) -> Live {
    Live {
        chart: chart.clone(),
        fan: format!("fan{}", channel.fan.fan_index),
        temp: channel.temp_input.map(|n| format!("temp{n}")),
    }
}

pub fn open(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Baseboard")
        .default_width(1000)
        .default_height(760)
        .build();
    if let Some((w, h)) = var("BASEBOARD_DEBUG_SIZE").and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    }) {
        window.set_default_size(w, h);
    }
    let numbers = Numbers::new(&crate::i18n::decimal_separator());
    let (_, detection, _) = crate::board::detect();
    let profile = detection.profile().cloned();
    let channels = profile
        .as_ref()
        .map(|p| channels(p, &numbers))
        .unwrap_or_default();

    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(2)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(12)
        .margin_end(12)
        .build();
    page.append(&flow);

    let shown = var("BASEBOARD_DEBUG_FAN_CHARTS").unwrap_or_default();
    let (with_cards, with_large) = (shown != "large", shown != "cards");
    let mut following = Vec::new();
    let mut cards = Vec::new();
    let mut named = Vec::new();
    for channel in channels.iter().filter(|_| with_cards) {
        let chart = FanChart::new(Size::Card, false);
        chart.set_curve(Some(channel.curve.clone()));
        flow.insert(&card(&channel.curve.name, &channel.fan.header, &chart), -1);
        cards.push(chart.clone());
        named.push((channel.fan.key.clone(), chart.clone()));
        following.push(live(&chart, channel));
    }

    let find = |key: &str| channels.iter().find(|c| c.fan.key == key);
    if let Some(sys1) = find("sys1").filter(|_| with_cards) {
        let chart = FanChart::new(Size::Card, false);
        chart.set_curve(Some(sys1.curve.clone()));
        chart.set_reading(Some(Reading {
            millidegrees: 46_000,
            duty: 76,
        }));
        flow.insert(&card(&sys1.curve.name, &sys1.fan.header, &chart), -1);
        cards.push(chart.clone());
        named.push(("moving".to_owned(), chart));
    }
    for (key, handles) in [("sys1", false), ("cpu1", true)] {
        let Some(channel) = find(key).filter(|_| with_large) else {
            continue;
        };
        let chart = FanChart::new(Size::Large, handles);
        chart.set_curve(Some(channel.curve.clone()));
        page.append(&card(&channel.curve.name, &channel.fan.header, &chart));
        named.push((format!("large-{key}"), chart.clone()));
        following.push(live(&chart, channel));
    }

    // Narrow windows: lower card charts, as in the mockups.
    let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        500.0,
        adw::LengthUnit::Sp,
    ));
    let compact = |on: bool| {
        let cards = cards.clone();
        move |_: &adw::Breakpoint| {
            for chart in &cards {
                chart.set_compact(on);
            }
        }
    };
    breakpoint.connect_apply(compact(true));
    breakpoint.connect_unapply(compact(false));
    window.add_breakpoint(breakpoint);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(
            &adw::Clamp::builder()
                // Charts as wide as in the mockups: 868 px large, about
                // 411 px in a card.
                .maximum_size(896)
                .tightening_threshold(896)
                .child(&page)
                .build(),
        )
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&scrolled));
    window.set_content(Some(&view));

    start_reading(&window, profile, following);
    if let Some(seconds) = var("BASEBOARD_DEBUG_HIDE_AFTER").and_then(|s| s.parse().ok()) {
        // A hidden window still belongs to the app, which keeps running.
        glib::timeout_add_seconds_local_once(
            seconds,
            glib::clone!(
                #[weak]
                window,
                move || {
                    window.set_visible(false);
                    eprintln!("baseboard: window hidden");
                }
            ),
        );
    }
    window.present();
    crate::application::debug_screenshot(app, window.upcast_ref(), move |window| {
        // Where each chart lies in the screenshot, in logical pixels.
        for (name, chart) in &named {
            if let Some(bounds) = chart.compute_bounds(window) {
                eprintln!(
                    "baseboard: chart {name} at {} {} {} {}",
                    bounds.x(),
                    bounds.y(),
                    bounds.width(),
                    bounds.height()
                );
            }
        }
    });
}

/// The sensor reader of the Overview, every second while the window is
/// visible and not suspended; each sample moves the operating points.
fn start_reading(window: &adw::ApplicationWindow, profile: Option<Profile>, following: Vec<Live>) {
    let chip = profile
        .as_ref()
        .map(|p| p.hwmon.name.clone())
        .unwrap_or_default();
    LIVE.with(|live| live.replace(following));
    let worker = Worker::start(
        Setup {
            root: config::sysfs_root(),
            board_name: None,
            profile,
            labels: None,
            cpu: None,
            options: Options {
                interval: Duration::from_secs(1),
                ..Options::default()
            },
            reads: FanReads::Nothing,
        },
        move |sample| {
            let chip = chip.clone();
            glib::idle_add_once(move || {
                LIVE.with(|live| {
                    for item in live.borrow().iter() {
                        item.chart.set_reading(reading(&sample.groups, &chip, item));
                    }
                });
            });
        },
    );
    let worker = Rc::new(worker);
    let running = move |window: &adw::ApplicationWindow| {
        worker.set_running(window.is_visible() && !window.is_suspended());
    };
    let running = Rc::new(running);
    let on_visible = running.clone();
    window.connect_visible_notify(move |w| on_visible(w));
    window.connect_suspended_notify(move |w| running(w));
}

fn reading(groups: &[Group], chip: &str, item: &Live) -> Option<Reading> {
    let rows = || {
        groups
            .iter()
            .flat_map(|g| g.rows.iter().chain(&g.auxiliary))
            .filter(|r| r.id.chip == chip)
    };
    let duty = rows()
        .find(|r| r.id.channel == item.fan)
        .and_then(|r| match &r.subtitle {
            Subtitle::Header { duty, .. } => *duty,
            _ => None,
        })?;
    let temp = item.temp.as_ref()?;
    let celsius = rows().find(|r| &r.id.channel == temp)?.value;
    Some(Reading {
        millidegrees: (celsius * 1000.0).round() as i64,
        duty: duty.round().clamp(0.0, 255.0) as u8,
    })
}
