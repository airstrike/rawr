//! What part of a window a capture shows, and at how many pixels a point
//! (#2009), asked for as `"region": {"x": .., "y": .., "width": ..,
//! "height": ..}` and `"scale": ..` beside a screenshot request.
//!
//! A region is in the window's logical points from its top-left corner,
//! title bar included, the space the input tools take positions in. A
//! region that runs past the window is cut to it; one that misses it is
//! refused. The scale defaults to the window's backing scale.
//!
//! The window draws its frame at its backing scale, and the capture
//! resamples that frame: each pixel of the PNG is the average of the
//! frame's pixels under it, weighted by how much of each it covers. A
//! scale below the backing scale averages detail away. One above it
//! repeats the frame's pixels, blending only at their edges, and adds no
//! detail the frame didn't draw.

use std::fmt;

use serde_json::{json, Value};

/// The largest scale a capture takes, in pixels a point.
const MOST: f64 = 4.0;

/// A rectangle in a window's points, from its top-left corner: its size is
/// more than zero each way and every side is a finite number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Region {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// What a capture asks to show: a region of the window, or all of it, at a
/// scale, or the window's backing scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Framing {
    region: Option<Region>,
    scale: Option<f64>,
}

/// A framing fitted to a window: the region cut to the window, the scale
/// it is drawn at, and the PNG's size in pixels, each at least one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fit {
    region: Region,
    scale: f64,
    /// The frame's pixels a point.
    backing: f64,
    width: usize,
    height: usize,
    /// Whether the PNG is the frame pixel for pixel: the whole window at
    /// its backing scale.
    whole: bool,
}

/// A frame's pixels as the window drew them: four bytes each, rows top
/// first, `stride` bytes apart.
pub(super) struct Source<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) stride: usize,
    pub(super) width: usize,
    pub(super) height: usize,
}

/// Why a framing was refused.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Error {
    /// `region` isn't an object of four numbers.
    Shape,
    /// `region`'s width or height is zero or less.
    Empty { width: f64, height: f64 },
    /// `scale` isn't a number more than zero and at most [`MOST`].
    Scale(Value),
    /// `region` misses the window, which is `width` × `height` points.
    Outside {
        region: Region,
        width: f64,
        height: f64,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape => formatter.write_str(
                "`region` needs `x`, `y`, `width` and `height`, each a number of points",
            ),
            Self::Empty { width, height } => write!(
                formatter,
                "`region` is {width} × {height} points: its width and height must be more than 0"
            ),
            Self::Scale(scale) => write!(
                formatter,
                "`scale` is {scale}: it must be a number of pixels a point more than 0 and at most {MOST}"
            ),
            Self::Outside {
                region,
                width,
                height,
            } => write!(
                formatter,
                "`region` at {}, {}, {} × {} points is outside the window, which is {width} × {height} points",
                region.x, region.y, region.width, region.height
            ),
        }
    }
}

impl Region {
    /// The region `value` names, a JSON object of four numbers.
    fn new(value: &Value) -> Result<Self, Error> {
        let number = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_f64)
                .filter(|number| number.is_finite())
                .ok_or(Error::Shape)
        };
        let region = Self {
            x: number("x")?,
            y: number("y")?,
            width: number("width")?,
            height: number("height")?,
        };
        if region.width > 0.0 && region.height > 0.0 {
            Ok(region)
        } else {
            Err(Error::Empty {
                width: region.width,
                height: region.height,
            })
        }
    }

    /// The part of this region inside a window of `width` × `height`
    /// points, or `None` when none of it is.
    fn within(self, width: f64, height: f64) -> Option<Self> {
        let left = self.x.max(0.0);
        let top = self.y.max(0.0);
        let right = (self.x + self.width).min(width);
        let bottom = (self.y + self.height).min(height);
        (right > left && bottom > top).then_some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }

    fn to_json(self) -> Value {
        json!({ "x": self.x, "y": self.y, "width": self.width, "height": self.height })
    }
}

impl Framing {
    /// The framing a screenshot request asks for: its `region` and `scale`,
    /// either absent or null for the whole window at its backing scale.
    pub(super) fn new(request: &Value) -> Result<Self, Error> {
        let region = match request.get("region") {
            None | Some(Value::Null) => None,
            Some(region) => Some(Region::new(region)?),
        };
        let scale = match request.get("scale") {
            None | Some(Value::Null) => None,
            Some(scale) => Some(
                scale
                    .as_f64()
                    .filter(|scale| *scale > 0.0 && *scale <= MOST)
                    .ok_or_else(|| Error::Scale(scale.clone()))?,
            ),
        };
        Ok(Self { region, scale })
    }

    /// Fit this framing to a window of `width` × `height` points drawn at
    /// `backing` pixels a point.
    pub(super) fn fit(&self, width: f64, height: f64, backing: f64) -> Result<Fit, Error> {
        let whole = Region {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let region = match self.region {
            None => whole,
            Some(region) => region.within(width, height).ok_or(Error::Outside {
                region,
                width,
                height,
            })?,
        };
        let scale = self.scale.unwrap_or(backing);
        let pixels = |points: f64| ((points * scale).round() as usize).max(1);
        Ok(Fit {
            region,
            scale,
            backing,
            width: pixels(region.width),
            height: pixels(region.height),
            whole: region == whole && scale == backing,
        })
    }
}

impl Fit {
    /// The PNG's width in pixels.
    pub(super) fn width(&self) -> usize {
        self.width
    }

    /// The PNG's height in pixels.
    pub(super) fn height(&self) -> usize {
        self.height
    }

    /// The PNG's pixels a point.
    pub(super) fn scale(&self) -> f64 {
        self.scale
    }

    /// The pixels a point the window drew its frame at.
    pub(super) fn backing(&self) -> f64 {
        self.backing
    }

    /// The region the PNG shows, in the window's points, as JSON.
    pub(super) fn region(&self) -> Value {
        self.region.to_json()
    }

    /// Whether the PNG is the frame the window drew, pixel for pixel.
    pub(super) fn is_whole(&self) -> bool {
        self.whole
    }

    /// The PNG's pixels drawn from `source`: four bytes each, in the
    /// source's order, rows top first and `width() * 4` bytes apart.
    pub(super) fn draw(&self, source: &Source<'_>) -> Vec<u8> {
        let columns = taps(
            self.region.x * self.backing,
            self.region.width * self.backing,
            self.width,
            source.width,
        );
        let rows = taps(
            self.region.y * self.backing,
            self.region.height * self.backing,
            self.height,
            source.height,
        );
        let mut pixels = Vec::with_capacity(self.width * self.height * 4);
        for row in &rows {
            for column in &columns {
                let mut sum = [0.0_f64; 4];
                for &(y, down) in row {
                    let line = &source.bytes[y * source.stride..];
                    for &(x, across) in column {
                        let weight = down * across;
                        for (channel, &value) in sum.iter_mut().zip(&line[x * 4..x * 4 + 4]) {
                            *channel += weight * f64::from(value);
                        }
                    }
                }
                pixels.extend(sum.map(|channel| channel.round().clamp(0.0, 255.0) as u8));
            }
        }
        pixels
    }
}

/// For each of `count` pixels along one axis, the source pixels under it
/// and the share of it each covers, summing to one: the span `length`
/// source pixels long from `start` is split into `count` equal parts, and
/// each part takes the source pixels it overlaps, within `limit`.
fn taps(start: f64, length: f64, count: usize, limit: usize) -> Vec<Vec<(usize, f64)>> {
    let step = length / count as f64;
    let last = limit.saturating_sub(1);
    (0..count)
        .map(|index| {
            let from = start + index as f64 * step;
            let to = from + step;
            let first = (from.floor().max(0.0) as usize).min(last);
            let end = (to.ceil().max(0.0) as usize).clamp(first + 1, limit.max(1));
            let mut weights: Vec<(usize, f64)> = (first..end)
                .map(|pixel| {
                    let pixel_start = pixel as f64;
                    let covered = to.min(pixel_start + 1.0) - from.max(pixel_start);
                    (pixel, covered.max(0.0))
                })
                .filter(|&(_, covered)| covered > 0.0)
                .collect();
            let total: f64 = weights.iter().map(|&(_, covered)| covered).sum();
            if total > 0.0 {
                for (_, covered) in &mut weights {
                    *covered /= total;
                }
                weights
            } else {
                // A part past the source's last pixel, from rounding the
                // region's edge to whole pixels, shows that last pixel.
                vec![(first, 1.0)]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame whose pixels are each `[v, v, v, 255]`.
    fn frame(values: &[u8]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|&value| [value, value, value, 255])
            .collect()
    }

    fn source(bytes: &[u8], width: usize) -> Source<'_> {
        Source {
            bytes,
            stride: width * 4,
            width,
            height: bytes.len() / (width * 4),
        }
    }

    fn reds(pixels: &[u8]) -> Vec<u8> {
        pixels.chunks(4).map(|pixel| pixel[0]).collect()
    }

    #[test]
    fn no_region_or_scale_is_the_whole_window_at_its_backing_scale() {
        let fit = Framing::new(&json!({}))
            .unwrap()
            .fit(400.0, 300.0, 2.0)
            .unwrap();
        assert_eq!((fit.width(), fit.height(), fit.scale()), (800, 600, 2.0));
        assert_eq!(
            fit.region(),
            json!({ "x": 0.0, "y": 0.0, "width": 400.0, "height": 300.0 })
        );
        assert!(fit.is_whole());
        let null = Framing::new(&json!({ "region": null, "scale": null })).unwrap();
        assert_eq!(null, Framing::default());
    }

    #[test]
    fn a_scale_sizes_the_png_by_points_not_by_the_backing_scale() {
        let framing = Framing::new(&json!({ "scale": 2 })).unwrap();
        let fit = framing.fit(1024.0, 800.0, 1.0).unwrap();
        assert_eq!((fit.width(), fit.height()), (2048, 1600));
        assert!(!fit.is_whole());
        // Half the window at twice the scale is the frame's size, but not
        // the frame.
        let half = Framing::new(&json!({
            "region": { "x": 0, "y": 0, "width": 512, "height": 400 },
            "scale": 2,
        }))
        .unwrap()
        .fit(1024.0, 800.0, 1.0)
        .unwrap();
        assert_eq!((half.width(), half.height()), (1024, 800));
        assert!(!half.is_whole());
        let fit = Framing::new(&json!({ "scale": 0.5 }))
            .unwrap()
            .fit(1024.0, 800.0, 2.0)
            .unwrap();
        assert_eq!((fit.width(), fit.height()), (512, 400));
    }

    #[test]
    fn a_region_past_the_window_is_cut_to_it() {
        let framing = Framing::new(&json!({
            "region": { "x": -10, "y": 250, "width": 60, "height": 100 },
        }))
        .unwrap();
        let fit = framing.fit(400.0, 300.0, 2.0).unwrap();
        assert_eq!(
            fit.region(),
            json!({ "x": 0.0, "y": 250.0, "width": 50.0, "height": 50.0 })
        );
        assert_eq!((fit.width(), fit.height()), (100, 100));
    }

    #[test]
    fn a_region_that_misses_the_window_is_refused() {
        let framing = Framing::new(&json!({
            "region": { "x": 400, "y": 0, "width": 10, "height": 10 },
        }))
        .unwrap();
        assert_eq!(
            framing.fit(400.0, 300.0, 1.0).unwrap_err().to_string(),
            "`region` at 400, 0, 10 × 10 points is outside the window, which is 400 × 300 points"
        );
    }

    #[test]
    fn bad_arguments_are_refused_with_what_they_need() {
        let refused = |request: Value| Framing::new(&request).unwrap_err().to_string();
        assert_eq!(
            refused(json!({ "region": { "x": 0, "y": 0, "width": 0, "height": 10 } })),
            "`region` is 0 × 10 points: its width and height must be more than 0"
        );
        assert_eq!(
            refused(json!({ "region": { "x": 0, "y": 0, "width": "10", "height": 10 } })),
            "`region` needs `x`, `y`, `width` and `height`, each a number of points"
        );
        assert_eq!(
            refused(json!({ "region": [0, 0, 10, 10] })),
            "`region` needs `x`, `y`, `width` and `height`, each a number of points"
        );
        for scale in [json!(0), json!(-1), json!(4.5), json!("2")] {
            assert!(refused(json!({ "scale": scale })).starts_with("`scale` is "));
        }
    }

    #[test]
    fn half_the_backing_scale_averages_each_square_of_pixels() {
        // 4 × 2 pixels at 2 a point: a 2 × 1 point window.
        let bytes = frame(&[0, 100, 200, 40, 20, 60, 0, 80]);
        let fit = Framing::new(&json!({ "scale": 1 }))
            .unwrap()
            .fit(2.0, 1.0, 2.0)
            .unwrap();
        let pixels = fit.draw(&source(&bytes, 4));
        assert_eq!(reds(&pixels), [45, 80]);
        assert_eq!(pixels[3], 255);
    }

    #[test]
    fn twice_the_backing_scale_repeats_each_pixel() {
        let bytes = frame(&[10, 20, 30, 40]);
        let fit = Framing::new(&json!({ "scale": 2 }))
            .unwrap()
            .fit(2.0, 2.0, 1.0)
            .unwrap();
        let pixels = fit.draw(&source(&bytes, 2));
        assert_eq!(
            reds(&pixels),
            [10, 10, 20, 20, 10, 10, 20, 20, 30, 30, 40, 40, 30, 30, 40, 40]
        );
    }

    #[test]
    fn a_region_reads_only_its_own_pixels_and_skips_the_row_padding() {
        // 3 × 2 pixels, rows padded to 16 bytes.
        let mut bytes = vec![0_u8; 32];
        for (index, value) in [1_u8, 2, 3, 4, 5, 6].into_iter().enumerate() {
            let at = (index / 3) * 16 + (index % 3) * 4;
            bytes[at..at + 4].copy_from_slice(&[value, 0, 0, 255]);
        }
        let padded = Source {
            bytes: &bytes,
            stride: 16,
            width: 3,
            height: 2,
        };
        let fit = Framing::new(&json!({
            "region": { "x": 1, "y": 1, "width": 2, "height": 1 },
        }))
        .unwrap()
        .fit(3.0, 2.0, 1.0)
        .unwrap();
        assert!(!fit.is_whole());
        assert_eq!(reds(&fit.draw(&padded)), [5, 6]);
    }

    #[test]
    fn a_sliver_of_a_point_still_draws_one_pixel() {
        let bytes = frame(&[7, 9]);
        let fit = Framing::new(&json!({
            "region": { "x": 1.9, "y": 0, "width": 0.05, "height": 1 },
        }))
        .unwrap()
        .fit(2.0, 1.0, 1.0)
        .unwrap();
        assert_eq!((fit.width(), fit.height()), (1, 1));
        assert_eq!(reds(&fit.draw(&source(&bytes, 2))), [9]);
    }
}
