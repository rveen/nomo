//! Drawing a computed plot as an SVG.
//!
//! # Why the engine draws it, and not a chart library
//!
//! `nomo html` produces one self-contained file, and the golden suite compares
//! it byte for byte across native and WebAssembly. A JavaScript charting
//! library would break both: the file would need a script the reader must
//! trust, and the drawing would happen on the reader's machine with the
//! reader's floating point rather than here. Emitting the geometry as markup
//! keeps the picture in the same category as every other result — computed
//! once, deterministically, and identical everywhere.
//!
//! It also means the drawing goes through [`crate::math`] like everything else.
//! Choosing where to put an axis tick needs `log10`, `floor` and `powf`, and
//! those come from the vendored library rather than the host's, so two machines
//! cannot disagree about where the gridlines are.
//!
//! # No colours in the markup
//!
//! Structure is `currentColor` at reduced opacity and each curve takes a class
//! the stylesheet fills in. A worksheet is read on a white page and a dark one,
//! and a hard-coded axis colour is legible on exactly one of them.
//!
//! # Several curves
//!
//! A curve carries `plot-curve` and `plot-curve-N`, `N` counting from one and
//! wrapping at [`PALETTE`]. Which colour that is belongs to the two
//! stylesheets, for the reason above; what belongs here is only that the curves
//! are *distinguishable* and that the legend says which is which — so the
//! legend swatch takes the same class as the curve it names, and the two cannot
//! drift apart.

use super::number::{self, NumberFormat};
use crate::math;
use crate::plot::{Extent, PlotValue};
use crate::unit::UnitTable;

/// The drawing surface, in SVG user units.
const WIDTH: f64 = 640.0;
const HEIGHT: f64 = 380.0;
/// Room for the axis labels. The left margin is the wide one because a vertical
/// tick label is written beside the axis rather than under it.
const LEFT: f64 = 74.0;
const RIGHT: f64 = 18.0;
const TOP: f64 = 14.0;
const BOTTOM: f64 = 42.0;

/// The bottom strip that holds the legend, drawn only when there is more than
/// one curve. Below the drawing rather than inside it: a legend floated in a
/// corner sits on top of whichever curve happens to go there, and a plot's
/// corners are where an interesting curve usually is.
const LEGEND: f64 = 22.0;
/// The length of a legend entry's line sample.
const SWATCH: f64 = 18.0;

/// The radius of the mark drawn at a measured point.
const MARK: f64 = 2.6;
/// Up to how many points in a table are marked individually.
///
/// A measurement is a thing that happened and the mark is where it happened, so
/// the marks are the data and the line between them is the reading of it. Past
/// a couple of hundred the marks touch each other and stop saying anything, and
/// the drawing would carry a circle per point for a smear — so beyond this the
/// line is the picture. The threshold is well above every table in the SMath
/// corpus.
const MARKED: usize = 200;

/// How many distinct curve classes there are before they repeat.
///
/// Six: the Okabe–Ito colourblind-safe palette without its yellow and its
/// black, neither of which is legible on both a white page and a dark one — and
/// unlike the structure, a curve's colour cannot follow the page, because two
/// curves have to differ from each other as well as from the ground. A seventh
/// curve reuses the first colour, which is honest enough at that point: the
/// legend still names every one of them.
pub const PALETTE: usize = 6;

/// How many ticks to aim for. A target rather than a count: the step is rounded
/// to something a reader can do arithmetic with, so the number that fits varies.
const TICKS: usize = 5;

/// One axis, once its ticks have been chosen.
struct Axis {
    lo: f64,
    hi: f64,
    step: f64,
}

impl Axis {
    /// An axis over exactly this span, for the horizontal one.
    ///
    /// The span is what the worksheet asked for — `plot(f, 0 Hz, 200 kHz)` said
    /// where to look — so it is not rounded outwards. Rounding it would draw
    /// the curve floating in the middle of a wider frame and show a stretch of
    /// x the author did not ask about.
    fn over(lo: f64, hi: f64) -> Axis {
        Axis {
            lo,
            hi,
            step: nice_step(hi - lo),
        }
    }

    /// Fit an axis around the data, rounded outwards to whole ticks.
    ///
    /// The vertical one, where nobody chose the extent: rounding out to whole
    /// ticks is what puts the top of the curve under a labelled gridline
    /// instead of against the frame.
    ///
    /// A flat curve — every sample identical — has no extent to scale, so it is
    /// given one rather than dividing by zero: the line then reads across the
    /// middle of the chart, which is what it is.
    fn fit(lo: f64, hi: f64) -> Axis {
        // Exact: a curve is flat when every sample is the same bits, and
        // anything looser would pad an axis that has a real extent.
        #[allow(clippy::float_cmp)]
        let flat = lo == hi;
        let (lo, hi) = if flat {
            let pad = if lo == 0.0 { 1.0 } else { math::abs(lo) / 2.0 };
            (lo - pad, hi + pad)
        } else {
            (lo, hi)
        };
        let step = nice_step(hi - lo);
        Axis {
            lo: math::floor(lo / step) * step,
            hi: -math::floor(-hi / step) * step,
            step,
        }
    }

    /// Where `value` falls along the axis, as a fraction from `lo` to `hi`.
    fn fraction(&self, value: f64) -> f64 {
        (value - self.lo) / (self.hi - self.lo)
    }

    /// Every whole multiple of the step inside the axis.
    ///
    /// Counted as `first + i*step` rather than accumulated, so the last tick
    /// lands where it should instead of near it — the rule the samples
    /// themselves follow. A fitted axis begins on a multiple already; a span
    /// given by the worksheet generally does not, so the first tick is the
    /// first multiple inside it.
    fn ticks(&self) -> Vec<f64> {
        let first = -math::floor(-self.lo / self.step) * self.step;
        let mut out = Vec::new();
        let mut i = 0.0;
        loop {
            let at = first + i * self.step;
            // A hair of tolerance, or a tick that should land exactly on the
            // end falls off it by one bit of rounding.
            if at > self.hi + self.step / 1024.0 || out.len() > 64 {
                break;
            }
            out.push(at);
            i += 1.0;
        }
        out
    }
}

/// A step that is 1, 2 or 5 times a power of ten.
///
/// Ticks at 0.25 and 0.75 are ticks a reader has to decode. Rounding the
/// spacing to one of three shapes is what makes an axis readable, and it is
/// the one piece of taste in this file.
fn nice_step(span: f64) -> f64 {
    let raw = span / TICKS as f64;
    // Ordered first so a NaN span — which compares false against everything —
    // is caught here rather than falling through to `log10` of it.
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let magnitude = math::powf(10.0, math::floor(math::log10(raw)));
    let normalised = raw / magnitude;
    let multiple = if normalised <= 1.0 {
        1.0
    } else if normalised <= 2.0 {
        2.0
    } else if normalised <= 5.0 {
        5.0
    } else {
        10.0
    };
    magnitude * multiple
}

/// A coordinate, to two decimals.
///
/// Rounded because the markup is compared byte for byte and a full-precision
/// `f64` would put seventeen digits into every one of five hundred coordinates
/// for a difference no screen has the pixels to show. Negative zero is
/// normalised: it is the same point as zero and would otherwise render as
/// `-0.00` on one side of a rounding boundary and `0.00` on the other.
fn coord(x: f64) -> String {
    let rounded = (x * 100.0).round() / 100.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded:.2}")
}

/// Which way a tick label should hang, so that the outermost two stay on the
/// drawing instead of running off its edge.
fn anchor_near_edge(at: f64, lo: f64, hi: f64) -> &'static str {
    // Half a label's width, near enough: these are short numbers in an 11px
    // face, and the choice only has to be right at the two ends.
    const REACH: f64 = 24.0;
    if at - lo < REACH {
        "plot-start"
    } else if hi - at < REACH {
        "plot-end"
    } else {
        "plot-x"
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Draw a plot.
pub fn svg(plot: &PlotValue, units: &UnitTable, numbers: &NumberFormat) -> String {
    let Some((y_lo, y_hi)) = plot.y_range() else {
        // Nothing finite was sampled, so there is no chart to draw. Said in
        // words where the picture would have been, rather than showing an empty
        // frame that looks like a working plot of nothing.
        return String::from("<p class=\"note\">[plot: nothing finite to draw]</p>\n");
    };
    // An axis the worksheet chose is exactly what it chose; one nobody chose is
    // fitted to the data and rounded out to whole ticks. That is why the
    // vertical axis has always been fitted, and a table's horizontal one is in
    // the same position: no author picked it.
    let x_axis = match plot.extent {
        Extent::Chosen => Axis::over(plot.from, plot.to),
        Extent::Measured => Axis::fit(plot.from, plot.to),
    };
    let y_axis = Axis::fit(y_lo, y_hi);

    let plot_w = WIDTH - LEFT - RIGHT;
    let plot_h = HEIGHT - TOP - BOTTOM;
    let sx = |v: f64| LEFT + x_axis.fraction(v) * plot_w;
    // Screen y runs downwards and an ordinate runs upwards.
    let sy = |v: f64| TOP + (1.0 - y_axis.fraction(v)) * plot_h;

    // One curve needs no legend — its name is on the line above the picture —
    // so a single-curve plot is drawn exactly as it was before there could be
    // more than one.
    let legend = plot.series.len() > 1;
    let height = HEIGHT + if legend { LEGEND } else { 0.0 };

    let mut out = format!(
        "<figure class=\"plot\"><svg viewBox=\"0 0 {WIDTH:.0} {height:.0}\" \
         role=\"img\" aria-label=\"{}\">\n",
        escape(&aria_label(plot))
    );

    // Gridlines and tick labels, before the curve so the curve sits on top.
    for at in x_axis.ticks() {
        let place = sx(at);
        let x = coord(place);
        out.push_str(&format!(
            "<line class=\"plot-grid\" x1=\"{x}\" y1=\"{}\" x2=\"{x}\" y2=\"{}\"/>\n",
            coord(TOP),
            coord(TOP + plot_h)
        ));
        // A label centred on the last tick hangs off the drawing, and `200000`
        // came out as `20000`. The two outermost are pulled inside instead, so
        // a span whose end lands on a tick still shows the number it ends at.
        out.push_str(&format!(
            "<text class=\"plot-label {}\" x=\"{x}\" y=\"{}\">{}</text>\n",
            anchor_near_edge(place, LEFT, LEFT + plot_w),
            coord(TOP + plot_h + 18.0),
            escape(&number::format(at, numbers))
        ));
    }
    for at in y_axis.ticks() {
        let y = coord(sy(at));
        out.push_str(&format!(
            "<line class=\"plot-grid\" x1=\"{}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\"/>\n",
            coord(LEFT),
            coord(LEFT + plot_w)
        ));
        out.push_str(&format!(
            "<text class=\"plot-label plot-y\" x=\"{}\" y=\"{}\">{}</text>\n",
            coord(LEFT - 8.0),
            coord(sy(at) + 4.0),
            escape(&number::format(at, numbers))
        ));
    }

    // The axes themselves, drawn over the grid.
    out.push_str(&format!(
        "<line class=\"plot-axis\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
        coord(LEFT),
        coord(TOP + plot_h),
        coord(LEFT + plot_w),
        coord(TOP + plot_h)
    ));
    out.push_str(&format!(
        "<line class=\"plot-axis\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
        coord(LEFT),
        coord(TOP),
        coord(LEFT),
        coord(TOP + plot_h)
    ));

    // The unit each axis is measured in, where a reader looks for it.
    if let Some(symbol) = unit_symbol(&plot.x_dim, units) {
        out.push_str(&format!(
            "<text class=\"plot-unit plot-end\" x=\"{}\" y=\"{}\">{}</text>\n",
            coord(LEFT + plot_w),
            coord(HEIGHT - 6.0),
            escape(&symbol)
        ));
    }
    if let Some(symbol) = unit_symbol(&plot.y_dim, units) {
        out.push_str(&format!(
            "<text class=\"plot-unit plot-y-title\" x=\"{}\" y=\"{}\">{}</text>\n",
            coord(LEFT),
            coord(TOP - 4.0),
            escape(&symbol)
        ));
    }

    // One polyline per run of finite samples. A run ends where the function
    // leaves the plane, and the gap is left open: joining across it would draw
    // a line through values the function never took.
    for (index, series) in plot.series.iter().enumerate() {
        let class = curve_class(index);
        let mut run: Vec<String> = Vec::new();
        let flush = |run: &mut Vec<String>, out: &mut String| {
            if run.len() > 1 {
                out.push_str(&format!(
                    "<polyline class=\"{class}\" points=\"{}\"/>\n",
                    run.join(" ")
                ));
            }
            run.clear();
        };
        for (x, y) in &series.points {
            if !x.is_finite() || !y.is_finite() {
                flush(&mut run, &mut out);
                continue;
            }
            run.push(format!("{},{}", coord(sx(*x)), coord(sy(*y))));
        }
        flush(&mut run, &mut out);

        // Where each measurement actually is, over the line that reads them.
        // An open ring rather than a disc, so it takes the curve's own class:
        // `plot-curve` already says fill nothing and stroke in this colour, and
        // a marker with a fill of its own would need a second palette to keep
        // in step with the first.
        if plot.extent == Extent::Measured && series.points.len() <= MARKED {
            for (x, y) in &series.points {
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                out.push_str(&format!(
                    "<circle class=\"{class} plot-mark\" cx=\"{}\" cy=\"{}\" r=\"{MARK}\"/>\n",
                    coord(sx(*x)),
                    coord(sy(*y))
                ));
            }
        }
    }

    if legend {
        out.push_str(&legend_strip(plot));
    }

    out.push_str("</svg></figure>\n");
    out
}

/// The classes one curve is drawn with.
fn curve_class(index: usize) -> String {
    format!("plot-curve plot-curve-{}", index % PALETTE + 1)
}

/// The row of name-and-sample pairs under the drawing.
///
/// Laid out left to right from an estimate of how wide a name will be, in the
/// same spirit as `anchor_near_edge`'s reach: the only thing the estimate
/// decides is spacing, and these are short names in an 11px face. A row that
/// would run past the right edge is left to run — truncating a curve's name
/// would make the legend lie about what is drawn, which is worse than a wide
/// picture.
fn legend_strip(plot: &PlotValue) -> String {
    // A little over half the font size: an 11px sans face averages near this
    // across the letters and digits a function name is made of.
    const ADVANCE: f64 = 6.4;
    const GAP: f64 = 18.0;

    let mut out = String::new();
    let mut x = LEFT;
    let baseline = HEIGHT + LEGEND / 2.0 + 4.0;
    for (index, series) in plot.series.iter().enumerate() {
        let sample_y = coord(baseline - 4.0);
        out.push_str(&format!(
            "<line class=\"{}\" x1=\"{}\" y1=\"{sample_y}\" x2=\"{}\" y2=\"{sample_y}\"/>\n",
            curve_class(index),
            coord(x),
            coord(x + SWATCH)
        ));
        out.push_str(&format!(
            "<text class=\"plot-label plot-legend\" x=\"{}\" y=\"{}\">{}</text>\n",
            coord(x + SWATCH + 5.0),
            coord(baseline),
            escape(&series.name)
        ));
        x += SWATCH + 5.0 + series.name.chars().count() as f64 * ADVANCE + GAP;
    }
    out
}

/// What a screen reader is told the picture is.
fn aria_label(plot: &PlotValue) -> String {
    let names: Vec<&str> = plot.series.iter().map(|s| s.name.as_str()).collect();
    match plot.extent {
        Extent::Chosen => format!("plot of {}", names.join(", ")),
        Extent::Measured => format!("plot of measured points: {}", names.join(", ")),
    }
}

fn unit_symbol(dim: &crate::dim::Dimension, units: &UnitTable) -> Option<String> {
    if dim.is_dimensionless() {
        return None;
    }
    Some(match units.preferred_for(dim) {
        Some(u) => u.symbol.clone(),
        None => dim.to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_one_two_or_five_times_a_power_of_ten() {
        for span in [1.0, 3.0, 7.0, 0.004, 170000.0, 1e-9, 9.9e12] {
            let step = nice_step(span);
            let magnitude = math::powf(10.0, math::floor(math::log10(step)));
            let shape = step / magnitude;
            assert!(
                (shape - 1.0).abs() < 1e-9
                    || (shape - 2.0).abs() < 1e-9
                    || (shape - 5.0).abs() < 1e-9,
                "span {span} gave step {step}, shape {shape}"
            );
        }
    }

    #[test]
    fn a_degenerate_span_still_produces_a_step() {
        // Never zero, or the axis would divide by it.
        assert!(nice_step(0.0) > 0.0);
        assert!(nice_step(f64::NAN) > 0.0);
        assert!(nice_step(f64::INFINITY) > 0.0);
    }

    #[test]
    fn the_horizontal_axis_is_exactly_the_span_that_was_asked_for() {
        let a = Axis::over(-3.0, 3.0);
        assert_eq!((a.lo, a.hi), (-3.0, 3.0));
        assert_eq!(a.fraction(-3.0), 0.0);
        assert_eq!(a.fraction(3.0), 1.0);
        // And its ticks are the whole multiples of a readable step that fall
        // inside it — not its ends, which are wherever the author put them.
        assert_eq!(a.ticks(), vec![-2.0, 0.0, 2.0]);
    }

    #[test]
    fn ticks_start_inside_a_span_that_does_not_begin_on_one() {
        let a = Axis::over(30000.0, 200000.0);
        let ticks = a.ticks();
        assert!(ticks.iter().all(|t| *t >= a.lo && *t <= a.hi), "{ticks:?}");
        assert!(ticks.len() >= 3, "{ticks:?}");
    }

    #[test]
    fn an_axis_encloses_its_data() {
        let a = Axis::fit(-2.0, 7.0);
        assert!(a.lo <= -2.0 && a.hi >= 7.0, "{} {}", a.lo, a.hi);
        // And the ends are whole ticks, which is what makes the labels read.
        assert_eq!(a.lo % a.step, 0.0);
    }

    #[test]
    fn a_flat_curve_gets_an_extent_rather_than_a_division_by_zero() {
        let a = Axis::fit(5.0, 5.0);
        assert!(a.hi > a.lo);
        assert!(a.fraction(5.0).is_finite());
    }

    #[test]
    fn the_outermost_tick_labels_hang_inwards() {
        assert_eq!(anchor_near_edge(74.0, 74.0, 622.0), "plot-start");
        assert_eq!(anchor_near_edge(622.0, 74.0, 622.0), "plot-end");
        assert_eq!(anchor_near_edge(348.0, 74.0, 622.0), "plot-x");
    }

    #[test]
    fn a_curve_and_its_legend_swatch_share_a_class() {
        // The legend is only useful if it names the colour it is beside, and
        // nothing else in the drawing ties the two together.
        assert_eq!(curve_class(0), "plot-curve plot-curve-1");
        assert_eq!(curve_class(PALETTE), curve_class(0));
        assert_eq!(curve_class(PALETTE + 1), curve_class(1));
    }

    #[test]
    fn negative_zero_is_written_as_zero() {
        assert_eq!(coord(-0.0), "0.00");
        assert_eq!(coord(-0.001), "0.00");
    }
}
