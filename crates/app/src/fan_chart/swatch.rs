// SPDX-License-Identifier: GPL-3.0-or-later

//! The small drawings of the legend under the chart of the page "Curve
//! Details": each mark of the chart, drawn with its colors and lines.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, graphene, gsk};

use super::{Palette, alpha, circle, line, stroke};

/// A mark of the chart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mark {
    /// The hatched zone below the header's minimum.
    #[default]
    BelowMinimum,
    /// The dashed line of the emergency threshold.
    Threshold,
    /// The dot of the operating point.
    Now,
    /// The dashed curve beyond its end points.
    Ends,
}

const WIDTH: i32 = 22;
const HEIGHT: i32 = 14;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Swatch {
        pub(super) mark: Cell<Mark>,
        pub(super) style_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Swatch {
        const NAME: &'static str = "BaseboardChartSwatch";
        type Type = super::Swatch;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("chart-swatch");
            klass.set_accessible_role(gtk::AccessibleRole::Presentation);
        }
    }

    impl ObjectImpl for Swatch {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            // Drawn again with the style, as the chart is.
            let style = adw::StyleManager::default();
            let redraw = |obj: &super::Swatch| {
                let obj = obj.downgrade();
                move |_: &adw::StyleManager| {
                    if let Some(obj) = obj.upgrade() {
                        obj.queue_draw();
                    }
                }
            };
            *self.style_handlers.borrow_mut() = vec![
                style.connect_dark_notify(redraw(&obj)),
                style.connect_high_contrast_notify(redraw(&obj)),
                style.connect_accent_color_rgba_notify(redraw(&obj)),
            ];
        }

        fn dispose(&self) {
            let style = adw::StyleManager::default();
            for handler in self.style_handlers.take() {
                style.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for Swatch {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let size = match orientation {
                gtk::Orientation::Horizontal => WIDTH,
                _ => HEIGHT,
            };
            (size, size, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct Swatch(ObjectSubclass<imp::Swatch>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Swatch {
    pub fn new(mark: Mark) -> Self {
        let swatch: Self = glib::Object::new();
        swatch.imp().mark.set(mark);
        swatch
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let palette = Palette::of(self.upcast_ref());
        let (width, height) = (f64::from(self.width()), f64::from(self.height()));
        let middle = height / 2.0;
        match self.imp().mark.get() {
            Mark::BelowMinimum => {
                // As on the chart: diagonal lines 6 px apart under a dashed
                // line.
                let (top, bottom) = (2.0, height - 1.0);
                snapshot.push_clip(&graphene::Rect::new(
                    0.0,
                    top as f32,
                    width as f32,
                    (bottom - top) as f32,
                ));
                let builder = gsk::PathBuilder::new();
                let step = 6.0 * std::f64::consts::SQRT_2;
                let mut x = -(bottom - top);
                while x < width {
                    builder.move_to(x as f32, bottom as f32);
                    builder.line_to((x + bottom - top) as f32, top as f32);
                    x += step;
                }
                snapshot.append_stroke(
                    &builder.to_path(),
                    &stroke(1.0, &[], false),
                    &alpha(&palette.fg, 0.2),
                );
                snapshot.pop();
                line(
                    snapshot,
                    (0.0, top),
                    (width, top),
                    &alpha(&palette.fg, 0.4),
                    &stroke(1.0, &[3.0, 3.0], false),
                );
            }
            Mark::Threshold => line(
                snapshot,
                (width / 2.0, 0.0),
                (width / 2.0, height),
                &palette.error,
                &stroke(1.5, &[5.0, 3.0], false),
            ),
            Mark::Now => {
                // The dot of the chart, a little smaller, with its halo.
                let center = (width / 2.0, middle);
                snapshot.append_fill(
                    &circle(center, 6.5),
                    gsk::FillRule::Winding,
                    &alpha(&palette.live, 0.2),
                );
                let dot = circle(center, 3.5);
                snapshot.append_fill(&dot, gsk::FillRule::Winding, &palette.live);
                snapshot.append_stroke(&dot, &stroke(1.5, &[], false), &palette.surface);
            }
            Mark::Ends => line(
                snapshot,
                (0.0, middle),
                (width, middle),
                &alpha(&palette.accent, 0.75),
                &stroke(2.25, &[5.0, 5.0], false),
            ),
        }
    }
}
