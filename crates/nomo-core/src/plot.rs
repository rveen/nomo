//! What a plot is, once it has been computed.
//!
//! # A plot is a value, not a picture
//!
//! `plot(f, a, b)` evaluates to this: the samples, their dimensions, and the
//! span they were taken over. Drawing is the renderer's job, and it is done
//! twice — as an SVG in `nomo html` and as one summary line in the text
//! output — from the same numbers. Nothing here knows what a pixel is.
//!
//! That is also why a plot is not a [`crate::resource::Image`]. A figure is
//! scanned evidence that the worksheet carries; a plot is a *result*, recomputed
//! whenever an input above it changes, and `resource.rs` says in as many words
//! that an image cannot be produced by an expression. These are different kinds
//! of thing that happen to end up looking similar on the page.
//!
//! # A fixed number of samples
//!
//! [`SAMPLES`] points, always, whatever the span. This is `integral`'s rule —
//! a fixed amount of work rather than a tolerance test — and it buys the same
//! two things: the drawing terminates, and it is the same drawing on every
//! machine, which is what lets a plot into the golden suite at all.
//!
//! It costs the thing fixed sampling always costs: a feature narrower than the
//! sample spacing can fall between two samples and not be drawn. A resonance
//! peak an engineer cares about is exactly such a feature. The honest answer is
//! that this is the same trade `integral` already makes with its fixed panels,
//! and that the way to see a narrow peak is to plot a narrower span — which is
//! what a person does with a chart anyway.
//!
//! # A measured table is the other kind of plot
//!
//! `plot(m)` takes an n×2 matrix — x in the first column, y in the second, the
//! shape `augment(x, y)` builds and the shape the SMath corpus plots — and
//! draws the points it holds. It is the same value: a series whose points came
//! from a table rather than from sampling a function, which is why [`Series`]
//! carries both coordinates rather than only the ordinate.
//!
//! The difference that survives to the drawing is where the horizontal extent
//! came from, and [`Extent`] records it. A worksheet that wrote `plot(f, 30 kHz,
//! 200 kHz)` chose that stretch of x and the axis is exactly it; a table chose
//! nothing, so its axis is fitted to the data and rounded out to whole ticks,
//! which is what the vertical axis has always done for the same reason.
//!
//! # Nodes are `from + i*step`
//!
//! Never repeated addition. The rule `range` and `integral` both follow, for
//! the reason given in the language reference: ten additions of `0.1` reach
//! `0.9999999999999999` where ten times `0.1` is exactly `1`, and the last
//! sample of a plot must land on the end of the span rather than near it.

use crate::dim::Dimension;

/// How many points a plot is sampled at.
///
/// One more than a power of two, so that both ends of the span are sampled and
/// so is its midpoint. 257 across a chart drawn a few hundred pixels wide is
/// finer than the pixels, which is as much resolution as a reader can be shown;
/// more would enlarge every snapshot for a smoothness nobody can see.
pub const SAMPLES: usize = 257;

/// Where a plot's horizontal extent came from.
///
/// Two cases and no third: a plot is drawn from named functions over a span the
/// worksheet gave, or from tables that brought their own x. The renderer asks
/// this rather than guessing from the shape of the data, because the answer is
/// about what the author chose and not about what the numbers look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    /// The worksheet named both ends, and the axis is exactly them.
    Chosen,
    /// The data did, and the axis is fitted to it.
    Measured,
}

/// One curve: the name it was drawn from, and its points in base SI.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// The function's name, or the name the table was written under — which is
    /// what a legend has to say either way.
    pub name: String,
    /// `(x, y)` in base SI, in the order they are drawn in. A sampled curve
    /// holds [`SAMPLES`] of them at `from + i*step`; a table holds its rows.
    ///
    /// Both coordinates, rather than ordinates against a shared span, because a
    /// table's x is its own — and one shape for both kinds keeps the renderer
    /// from having two ways to ask where a point is.
    ///
    /// Non-finite entries are kept rather than dropped: a curve that leaves the
    /// plane is something the reader is shown a gap for, not something quietly
    /// closed over.
    pub points: Vec<(f64, f64)>,
}

/// A computed plot.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotValue {
    /// The span, in base SI. For a table this is the extent of its own x.
    pub from: f64,
    pub to: f64,
    /// Whether the worksheet chose that span or the data did.
    pub extent: Extent,
    pub x_dim: Dimension,
    /// Every series shares this: they were all drawn against one vertical axis,
    /// so a plot of a length beside a plot of a force is refused when it is
    /// built rather than drawn with two meanings on one axis.
    pub y_dim: Dimension,
    pub series: Vec<Series>,
    /// A logarithmic horizontal axis, and with it logarithmic *sampling*.
    ///
    /// Not only a drawing decision, which is why it is here rather than in the
    /// renderer: a decade sweep sampled linearly puts nine tenths of its points
    /// in the last decade and almost none in the first, so the low end of a
    /// Bode plot would be drawn from four samples however finely the rest was
    /// resolved. See [`PlotValue::x_at`].
    pub x_log: bool,
    /// A logarithmic vertical axis. Drawing only — the ordinates are whatever
    /// the function returned.
    pub y_log: bool,
    /// The window to draw, when the worksheet asked for one, in base SI.
    ///
    /// Separate from `from`/`to`, which say what was *sampled*: `axis` chooses
    /// what is shown and the span chooses what is computed, and conflating them
    /// would make zooming a chart silently change the curve.
    pub x_limits: Option<(f64, f64)>,
    pub y_limits: Option<(f64, f64)>,
}

/// How the axes of the next plot are drawn.
///
/// Carried by the evaluator and copied into each plot as it is built, so that a
/// worksheet's `axis` lines apply to the plots below them the way `digits`
/// applies to the results below it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Axes {
    pub x_log: bool,
    pub y_log: bool,
    pub x_limits: Option<(f64, f64)>,
    pub y_limits: Option<(f64, f64)>,
}

impl PlotValue {
    /// The abscissa of sample `i`, by the same formula everywhere it is needed.
    ///
    /// A method rather than a stored vector so that the renderer and the
    /// evaluator cannot drift apart on it: there is one expression for where a
    /// sample is, and both use it.
    pub fn x_at(&self, i: usize) -> f64 {
        if self.x_log {
            // Geometric rather than arithmetic spacing, by the same `a + i*step`
            // rule one level up: the exponent is what advances in equal steps,
            // so the samples land evenly along the axis they will be drawn on.
            // Both ends are positive — plot refuses a logarithmic span that
            // touches zero — so the logarithms exist.
            let (lo, hi) = (crate::math::ln(self.from), crate::math::ln(self.to));
            let step = (hi - lo) / (SAMPLES - 1) as f64;
            return crate::math::exp(lo + (i as f64) * step);
        }
        let step = (self.to - self.from) / (SAMPLES - 1) as f64;
        self.from + (i as f64) * step
    }

    /// The smallest and largest finite value across every series.
    ///
    /// `None` when nothing finite was sampled — a plot of a function that is
    /// infinite everywhere on the span has no vertical extent to draw, and
    /// saying so beats inventing one.
    pub fn y_range(&self) -> Option<(f64, f64)> {
        Self::range(
            self.series
                .iter()
                .flat_map(|s| s.points.iter().map(|p| p.1)),
        )
    }

    /// The same, along the other axis. What a table's own extent is.
    pub fn x_range(&self) -> Option<(f64, f64)> {
        Self::range(
            self.series
                .iter()
                .flat_map(|s| s.points.iter().map(|p| p.0)),
        )
    }

    /// The smallest and largest of a run of values, ignoring what is not
    /// finite. Public because a logarithmic axis asks it of the positive part
    /// of the data rather than of all of it.
    pub fn range(values: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
        let mut range: Option<(f64, f64)> = None;
        for value in values {
            if !value.is_finite() {
                continue;
            }
            range = Some(match range {
                None => (value, value),
                Some((lo, hi)) => (lo.min(value), hi.max(value)),
            });
        }
        range
    }

    /// How many drawn points were not finite, across every series.
    ///
    /// A point counts once however many of its two coordinates left the plane:
    /// what the reader sees either way is one place the line is not.
    ///
    /// Reported rather than silently skipped: a plot with a hole in it is a
    /// fact about the function, and the text renderer says so on the line.
    pub fn gaps(&self) -> usize {
        self.series
            .iter()
            .flat_map(|s| &s.points)
            .filter(|(x, y)| !x.is_finite() || !y.is_finite())
            .count()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn plot(ordinates: Vec<f64>) -> PlotValue {
        PlotValue {
            from: 0.0,
            to: 1.0,
            extent: Extent::Chosen,
            x_dim: Dimension::DIMENSIONLESS,
            y_dim: Dimension::DIMENSIONLESS,
            x_log: false,
            y_log: false,
            x_limits: None,
            y_limits: None,
            series: vec![Series {
                name: "f".into(),
                points: ordinates
                    .into_iter()
                    .enumerate()
                    .map(|(i, y)| (i as f64, y))
                    .collect(),
            }],
        }
    }

    #[test]
    fn the_last_sample_lands_exactly_on_the_end() {
        // The property `from + i*step` is chosen for: repeated addition of the
        // step arrives near the end rather than on it.
        let p = PlotValue {
            from: 0.0,
            to: 1.0,
            ..plot(vec![])
        };
        assert_eq!(p.x_at(0), 0.0);
        assert_eq!(p.x_at(SAMPLES - 1), 1.0);
        assert_eq!(p.x_at((SAMPLES - 1) / 2), 0.5);
    }

    #[test]
    fn a_span_that_does_not_start_at_zero_still_ends_where_it_should() {
        let p = PlotValue {
            from: 30000.0,
            to: 200000.0,
            ..plot(vec![])
        };
        assert_eq!(p.x_at(0), 30000.0);
        assert_eq!(p.x_at(SAMPLES - 1), 200000.0);
    }

    #[test]
    fn the_range_ignores_what_is_not_finite() {
        let p = plot(vec![1.0, f64::NAN, 3.0, f64::INFINITY, -2.0]);
        assert_eq!(p.y_range(), Some((-2.0, 3.0)));
        assert_eq!(p.gaps(), 2);
    }

    #[test]
    fn a_table_reports_the_extent_of_its_own_x() {
        let p = PlotValue {
            extent: Extent::Measured,
            series: vec![Series {
                name: "data".into(),
                points: vec![(2.0, 1.0), (5.0, 4.0), (3.0, 9.0)],
            }],
            ..plot(vec![])
        };
        assert_eq!(p.x_range(), Some((2.0, 5.0)));
        assert_eq!(p.y_range(), Some((1.0, 9.0)));
    }

    #[test]
    fn a_point_that_left_the_plane_counts_once() {
        let p = PlotValue {
            extent: Extent::Measured,
            series: vec![Series {
                name: "data".into(),
                points: vec![(f64::NAN, f64::NAN), (1.0, 2.0)],
            }],
            ..plot(vec![])
        };
        assert_eq!(p.gaps(), 1);
    }

    #[test]
    fn a_curve_with_nothing_finite_on_it_has_no_range() {
        let p = plot(vec![f64::NAN, f64::INFINITY]);
        assert_eq!(p.y_range(), None);
        assert_eq!(p.gaps(), 2);
    }
}
