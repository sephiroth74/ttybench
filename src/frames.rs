//! The frames of a window's capture and what changed between two of them:
//! the line that the typed keys show on, found of a few keys' changes, and
//! the frame where a key's character shows, the first displayed after the
//! key whose line differs from the frame displayed before it.
//!
//! A frame holds a part of the window, a band of whole rows of pixels or
//! all of it, its pixels BGRA, as the capture gives them, placed in the
//! window's pixels from its top left.

/// How much a channel of a pixel changes, at least, for the pixel to have
/// changed, out of 255: a glyph's edge changes by far more, a color's
/// conversion by less.
pub const TOLERANCE: u8 = 16;

/// How many pixels change, at least, for a part of a frame to have changed.
pub const LEAST_PIXELS: usize = 4;

/// A rectangle of a window's pixels, from its top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Its left edge.
    pub x: usize,
    /// Its top edge.
    pub y: usize,
    /// Its width.
    pub width: usize,
    /// Its height.
    pub height: usize,
}

impl Rect {
    /// The column after its right edge.
    pub fn right(&self) -> usize {
        self.x + self.width
    }

    /// The row after its bottom edge.
    pub fn bottom(&self) -> usize {
        self.y + self.height
    }

    /// Where it meets `other`, if they meet.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then_some(Rect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

/// A frame of a window's capture, or a band of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// When the window server displayed it, in nanoseconds of the shared
    /// clock.
    pub display_ns: u64,
    /// When the capture gave it to the tool, in nanoseconds of the shared
    /// clock.
    pub arrived_ns: u64,
    /// The part of the window that it holds.
    pub area: Rect,
    /// Its pixels, BGRA, row after row of `area`'s width.
    pub pixels: Vec<u8>,
}

impl Frame {
    /// A frame of `area`, of one color, BGRA.
    pub fn filled(area: Rect, color: [u8; 4], display_ns: u64) -> Self {
        Self {
            display_ns,
            arrived_ns: display_ns,
            area,
            pixels: color.repeat(area.width * area.height),
        }
    }

    /// The pixel at `x`, `y` of the window, if the frame holds it.
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        let inside =
            x >= self.area.x && x < self.area.right() && y >= self.area.y && y < self.area.bottom();
        if !inside {
            return None;
        }
        let at = ((y - self.area.y) * self.area.width + (x - self.area.x)) * 4;
        let pixel = self.pixels.get(at..at + 4)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }

    /// The part of it within `rect`, if it holds any.
    pub fn crop(&self, rect: Rect) -> Option<Frame> {
        let area = rect.intersect(&self.area)?;
        let mut pixels = Vec::with_capacity(area.width * area.height * 4);
        for y in area.y..area.bottom() {
            let start = ((y - self.area.y) * self.area.width + (area.x - self.area.x)) * 4;
            pixels.extend_from_slice(self.pixels.get(start..start + area.width * 4)?);
        }
        Some(Frame {
            display_ns: self.display_ns,
            arrived_ns: self.arrived_ns,
            area,
            pixels,
        })
    }

    /// Paints `rect` of `color`, where the frame holds it, as a test draws.
    pub fn paint(&mut self, rect: Rect, color: [u8; 4]) {
        let Some(rect) = rect.intersect(&self.area) else {
            return;
        };
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let at = ((y - self.area.y) * self.area.width + (x - self.area.x)) * 4;
                self.pixels[at..at + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Whether two pixels differ, a channel by [`TOLERANCE`] at least.
fn differ(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).any(|(&a, b)| a.abs_diff(b) >= TOLERANCE)
}

/// The box of the pixels that differ between `a` and `b` within `within`,
/// or within all that both hold of none; none if fewer than
/// [`LEAST_PIXELS`] differ.
pub fn changed_box(a: &Frame, b: &Frame, within: Option<Rect>) -> Option<Rect> {
    let mut area = a.area.intersect(&b.area)?;
    if let Some(within) = within {
        area = area.intersect(&within)?;
    }
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
    let mut count = 0;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let (Some(one), Some(other)) = (a.pixel(x, y), b.pixel(x, y)) else {
                continue;
            };
            if differ(one, other) {
                count += 1;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    (count >= LEAST_PIXELS).then(|| Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// The line that the typed keys show on, in the window's pixels: the band
/// that holds it, wide enough for a line's keys, and the place of its
/// cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// The band of the line's rows, from a cell before its first to two
    /// after its last.
    pub band: Rect,
    /// The left edge of its first cell's change.
    pub first: f64,
    /// The width of a cell.
    pub pitch: f64,
}

/// The median of `values`, none of none.
fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

/// The line of `boxes`, each the change that a key typed in turn made,
/// from a line's first cell, in a window of `width` by `height` pixels, of
/// `keys` keys a line; why none otherwise. Each box holds the cell that the
/// key's character took and the next, that the cursor took; their medians
/// pass over a box of a change elsewhere.
pub fn line_of(boxes: &[Rect], keys: usize, width: usize, height: usize) -> Result<Line, String> {
    if boxes.len() < 3 {
        return Err(format!(
            "{} keys showed a change, of 3 at least",
            boxes.len()
        ));
    }
    let top = median(&mut boxes.iter().map(|b| b.y as f64).collect::<Vec<_>>()).unwrap_or(0.0);
    let bottom =
        median(&mut boxes.iter().map(|b| b.bottom() as f64).collect::<Vec<_>>()).unwrap_or(0.0);
    let pitch = median(
        &mut boxes
            .windows(2)
            .map(|pair| pair[1].x as f64 - pair[0].x as f64)
            .collect::<Vec<_>>(),
    )
    .unwrap_or(0.0);
    if pitch < 1.0 || pitch > width as f64 / 8.0 || bottom <= top {
        return Err(format!(
            "the typed keys moved by {pitch:.1} pixels, {:.1} high, as no line of characters",
            bottom - top
        ));
    }
    let first = median(
        &mut boxes
            .iter()
            .enumerate()
            .map(|(at, b)| b.x as f64 - at as f64 * pitch)
            .collect::<Vec<_>>(),
    )
    .unwrap_or(0.0);
    let margin = ((bottom - top) / 4.0).ceil();
    let left = (first - pitch).max(0.0).floor() as usize;
    let right = ((first + (keys as f64 + 2.0) * pitch).ceil() as usize).min(width);
    let up = (top - margin).max(0.0) as usize;
    let down = ((bottom + margin) as usize).min(height);
    if right <= left || down <= up {
        return Err("the typed line lies outside the window".into());
    }
    Ok(Line {
        band: Rect {
            x: left,
            y: up,
            width: right - left,
            height: down - up,
        },
        first,
        pitch,
    })
}

/// A key typed, watched for its character: the frames that come are taken
/// in turn, until the first displayed after the key whose `area` differs
/// from the last displayed at or before it.
#[derive(Debug)]
pub struct Watch {
    key_ns: u64,
    area: Rect,
    before: Option<Frame>,
}

impl Watch {
    /// A key typed at `key_ns`, its character looked for in `area`, of the
    /// last frame that came before it, if one.
    pub fn new(key_ns: u64, area: Rect, before: Option<Frame>) -> Self {
        Self {
            key_ns,
            area,
            before,
        }
    }

    /// Takes the next frame that came: the display time of the frame
    /// where the character shows, once it shows. A frame displayed at or
    /// before the key is the one to compare the next with; so is the
    /// first of none before it.
    pub fn take(&mut self, frame: Frame) -> Option<u64> {
        if frame.display_ns <= self.key_ns {
            self.before = Some(frame);
            return None;
        }
        let Some(before) = &self.before else {
            self.before = Some(frame);
            return None;
        };
        let shown = changed_box(before, &frame, Some(self.area)).map(|_| frame.display_ns);
        self.before = Some(frame);
        shown
    }

    /// The last frame that it took, the one that the next key's frames
    /// compare with, if one.
    pub fn into_last(self) -> Option<Frame> {
        self.before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKGROUND: [u8; 4] = [30, 30, 30, 255];
    const INK: [u8; 4] = [220, 220, 220, 255];
    const CURSOR: [u8; 4] = [200, 160, 40, 255];

    /// A window of 800 by 400 pixels, of cells of 17 by 34 from 24, 180,
    /// a block cursor at `cursor` and the characters before it typed: a
    /// glyph of the cell's middle, as an ink's.
    fn screen(cursor: usize, display_ns: u64) -> Frame {
        let window = Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 400,
        };
        let mut frame = Frame::filled(window, BACKGROUND, display_ns);
        let cell = |at: usize| Rect {
            x: 24 + at * 17,
            y: 180,
            width: 17,
            height: 34,
        };
        for at in 0..cursor {
            let c = cell(at);
            frame.paint(
                Rect {
                    x: c.x + 3,
                    y: c.y + 8,
                    width: 11,
                    height: 18,
                },
                INK,
            );
        }
        frame.paint(cell(cursor), CURSOR);
        frame
    }

    #[test]
    fn rectangles_meet_where_they_overlap() {
        let a = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        let b = Rect {
            x: 5,
            y: 8,
            width: 10,
            height: 10,
        };
        assert_eq!(
            a.intersect(&b),
            Some(Rect {
                x: 5,
                y: 8,
                width: 5,
                height: 2
            })
        );
        let apart = Rect {
            x: 10,
            y: 0,
            width: 3,
            height: 3,
        };
        assert_eq!(a.intersect(&apart), None);
    }

    #[test]
    fn a_typed_key_changes_its_cell_and_the_next() {
        let before = screen(0, 0);
        let after = screen(1, 1);
        assert_eq!(
            changed_box(&before, &after, None),
            Some(Rect {
                x: 24,
                y: 180,
                width: 34,
                height: 34
            })
        );
        assert_eq!(changed_box(&before, &before.clone(), None), None);
    }

    #[test]
    fn a_change_below_the_tolerance_or_of_a_few_pixels_is_none() {
        let before = screen(0, 0);
        let mut faint = before.clone();
        faint.paint(
            Rect {
                x: 300,
                y: 20,
                width: 40,
                height: 40,
            },
            [40, 40, 40, 255],
        );
        assert_eq!(changed_box(&before, &faint, None), None);
        let mut speck = before.clone();
        speck.paint(
            Rect {
                x: 300,
                y: 20,
                width: 3,
                height: 1,
            },
            INK,
        );
        assert_eq!(changed_box(&before, &speck, None), None);
    }

    #[test]
    fn a_band_compares_with_the_whole_window_where_both_hold_it() {
        let whole = screen(2, 0);
        let rows = Rect {
            x: 0,
            y: 170,
            width: 800,
            height: 60,
        };
        let band = screen(3, 1).crop(rows).unwrap();
        assert_eq!(band.area, rows);
        assert_eq!(band.pixel(0, 169), None);
        let changed = changed_box(&whole, &band, None).unwrap();
        assert_eq!((changed.x, changed.width), (24 + 2 * 17, 34));
    }

    #[test]
    fn the_line_of_a_few_keys_is_their_row_and_their_cells() {
        let boxes: Vec<Rect> = (0..6)
            .map(|at| changed_box(&screen(at, 0), &screen(at + 1, 1), None).unwrap())
            .collect();
        let line = line_of(&boxes, 40, 800, 400).unwrap();
        assert_eq!(line.pitch, 17.0);
        assert_eq!(line.first, 24.0);
        assert_eq!(line.band.x, 7);
        assert_eq!(line.band.right(), 24 + 42 * 17);
        assert_eq!((line.band.y, line.band.bottom()), (180 - 9, 214 + 9));
    }

    #[test]
    fn a_box_of_a_change_elsewhere_passes_over_the_line() {
        let mut boxes: Vec<Rect> = (0..6)
            .map(|at| changed_box(&screen(at, 0), &screen(at + 1, 1), None).unwrap())
            .collect();
        // The first key also took a card away at the window's top.
        boxes[0] = Rect {
            x: 24,
            y: 10,
            width: 500,
            height: 204,
        };
        let line = line_of(&boxes, 40, 800, 400).unwrap();
        assert_eq!((line.first, line.pitch), (24.0, 17.0));
        assert_eq!(line.band.y, 171);
    }

    #[test]
    fn keys_that_showed_nothing_like_a_line_are_refused() {
        let same = Rect {
            x: 24,
            y: 180,
            width: 34,
            height: 34,
        };
        assert!(line_of(&[same, same, same], 40, 800, 400).is_err());
        assert!(line_of(&[same, same], 40, 800, 400).is_err());
    }

    #[test]
    fn a_key_shows_in_the_first_frame_after_it_that_differs() {
        let area = Rect {
            x: 0,
            y: 170,
            width: 800,
            height: 60,
        };
        let mut watch = Watch::new(100, area, Some(screen(3, 50)));
        // A frame displayed before the key, as a line's clearing, is the
        // one to compare with.
        assert_eq!(watch.take(screen(0, 90)), None);
        // A frame after the key of a change elsewhere is no character.
        let mut elsewhere = screen(0, 110);
        elsewhere.paint(
            Rect {
                x: 600,
                y: 10,
                width: 50,
                height: 20,
            },
            INK,
        );
        assert_eq!(watch.take(elsewhere), None);
        assert_eq!(watch.take(screen(1, 127)), Some(127));
        assert_eq!(watch.into_last().map(|frame| frame.display_ns), Some(127));
    }

    #[test]
    fn a_key_of_no_frame_before_it_compares_with_the_first_after() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 400,
        };
        let mut watch = Watch::new(100, area, None);
        assert_eq!(watch.take(screen(0, 110)), None);
        assert_eq!(watch.take(screen(1, 127)), Some(127));
    }
}
