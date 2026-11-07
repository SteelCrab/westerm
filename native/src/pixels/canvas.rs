//! The picture a pixel app has drawn with Kitty images, kept as one RGBA
//! frame. Images and placements follow Kitty rules: a placement sits at a
//! cell plus a pixel offset; a higher z covers a lower one, and among equal
//! z the newer one; sending an image again under the same id deletes the old
//! image and its placements. Changed areas are recomposed only when they are
//! taken, so many commands in one frame cost one composition.

use std::collections::HashMap;

/// Most rectangles one update carries; more are merged into their bounds.
pub const MAX_RECTS: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The terminal grid and its pixel size, as the PTY reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
}

impl Grid {
    /// Whole pixels per cell, as Kitty clients compute it from the PTY size.
    pub fn cell(&self) -> (u32, u32) {
        (self.width / self.cols.max(1), self.height / self.rows.max(1))
    }
}

/// Decoded RGBA pixels, 4 bytes each.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where and how to show an image.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub image: u32,
    /// Placement id; 0 for none.
    pub id: u32,
    pub row: u32,
    pub col: u32,
    /// Pixel offset inside the cell.
    pub dx: u32,
    pub dy: u32,
    /// Part of the image to show; None for all of it.
    pub src: Option<Rect>,
    pub z: i32,
}

impl Rect {
    fn area(&self) -> u64 {
        u64::from(self.w) * u64::from(self.h)
    }

    fn right(&self) -> u32 {
        self.x + self.w
    }

    fn bottom(&self) -> u32 {
        self.y + self.h
    }

    fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Rect { x, y, w: right - x, h: bottom - y })
    }

    fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect { x, y, w: self.right().max(other.right()) - x, h: self.bottom().max(other.bottom()) - y }
    }

    fn contains(&self, other: &Rect) -> bool {
        self.x <= other.x && self.y <= other.y && self.right() >= other.right() && self.bottom() >= other.bottom()
    }
}

struct Stored {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    /// Every pixel has alpha 255: it is copied, not blended.
    opaque: bool,
    seq: u64,
}

/// A placement. Its place in `Canvas::shown` gives its order.
struct Shown {
    place: Place,
}

pub struct Canvas {
    grid: Grid,
    budget: usize,
    frame: Vec<u8>,
    images: HashMap<u32, Stored>,
    /// Bottom to top: by z, then by age.
    shown: Vec<Shown>,
    /// Changed since the last finished frame; composed when it ends.
    dirty: Vec<Rect>,
    /// Composed and not yet taken.
    done: Vec<Rect>,
    used: usize,
    seq: u64,
}

impl Canvas {
    /// `budget`: most image bytes kept; the oldest images go first.
    pub fn new(grid: Grid, budget: usize) -> Self {
        let mut canvas = Self {
            grid,
            budget,
            frame: Vec::new(),
            images: HashMap::new(),
            shown: Vec::new(),
            dirty: Vec::new(),
            done: Vec::new(),
            used: 0,
            seq: 0,
        };
        canvas.resize(grid);
        canvas.dirty.clear();
        canvas
    }

    /// Frame size in pixels: whole cells only.
    pub fn size(&self) -> (u32, u32) {
        let (cell_w, cell_h) = self.grid.cell();
        (self.grid.cols * cell_w, self.grid.rows * cell_h)
    }

    /// Stores an image, replacing any image with the same id together with
    /// its placements.
    pub fn transmit(&mut self, id: u32, image: Image) {
        self.remove_image(id);
        let opaque = image.rgba.iter().skip(3).step_by(4).all(|&alpha| alpha == 255);
        self.used += image.rgba.len();
        self.seq += 1;
        let stored = Stored { width: image.width, height: image.height, rgba: image.rgba, opaque, seq: self.seq };
        self.images.insert(id, stored);
        self.evict(id);
    }

    /// Shows a stored image. False when there is no image with that id.
    pub fn place(&mut self, place: Place) -> bool {
        if !self.images.contains_key(&place.image) {
            return false;
        }
        if place.id != 0 {
            self.remove_shown(|shown| shown.image == place.image && shown.id == place.id);
        }
        // After every placement with the same z: the newest is on top.
        let at = self.shown.partition_point(|shown| shown.place.z <= place.z);
        self.shown.insert(at, Shown { place });
        if let Some(rect) = self.rect_of(&place) {
            self.mark(rect);
        }
        true
    }

    /// Deletes placements: of one image (and one placement id), or all of
    /// them. `free` also deletes the image data.
    pub fn delete(&mut self, image: Option<u32>, placement: Option<u32>, free: bool) {
        match image {
            None => {
                self.remove_shown(|_| true);
                if free {
                    self.images.clear();
                    self.used = 0;
                }
            }
            Some(id) if free => self.remove_image(id),
            Some(id) => self.remove_shown(|shown| shown.image == id && placement.is_none_or(|p| shown.id == p)),
        }
    }

    pub fn resize(&mut self, grid: Grid) {
        let (width, height) = self.size();
        if grid == self.grid && self.frame.len() == width as usize * height as usize * 4 {
            return;
        }
        self.grid = grid;
        let (width, height) = self.size();
        self.frame = vec![0; width as usize * height as usize * 4];
        // Areas taken from the old frame no longer exist.
        self.done.clear();
        self.dirty = vec![Rect { x: 0, y: 0, w: width, h: height }];
        self.dirty.retain(|rect| rect.area() > 0);
    }

    /// Something changed since the last frame was taken, finished or not.
    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty() || !self.done.is_empty()
    }

    /// Ends a frame: its changed areas are composed now, so later commands
    /// (the next frame) do not show up in it.
    pub fn finish_frame(&mut self) {
        for rect in std::mem::take(&mut self.dirty) {
            self.compose(rect);
            add_rect(&mut self.done, rect);
        }
    }

    /// A finished frame has areas not yet taken.
    pub fn has_finished(&self) -> bool {
        !self.done.is_empty()
    }

    /// The finished areas with their pixels.
    pub fn take_finished(&mut self) -> Vec<(Rect, Vec<u8>)> {
        let (width, _) = self.size();
        std::mem::take(&mut self.done)
            .into_iter()
            .map(|rect| {
                let mut pixels = Vec::with_capacity(rect.area() as usize * 4);
                for y in rect.y..rect.bottom() {
                    let from = (y as usize * width as usize + rect.x as usize) * 4;
                    pixels.extend_from_slice(&self.frame[from..from + rect.w as usize * 4]);
                }
                (rect, pixels)
            })
            .collect()
    }

    /// Finishes the frame and takes it.
    #[cfg(test)]
    pub fn take_dirty(&mut self) -> Vec<(Rect, Vec<u8>)> {
        self.finish_frame();
        self.take_finished()
    }

    /// Where a placement shows, cut to the frame.
    fn rect_of(&self, place: &Place) -> Option<Rect> {
        let image = self.images.get(&place.image)?;
        let full = Rect { x: 0, y: 0, w: image.width, h: image.height };
        let src = place.src.map_or(Some(full), |src| src.intersect(&full))?;
        let (cell_w, cell_h) = self.grid.cell();
        let x = place.col.checked_mul(cell_w)?.checked_add(place.dx)?;
        let y = place.row.checked_mul(cell_h)?.checked_add(place.dy)?;
        let (width, height) = self.size();
        Rect { x, y, w: src.w, h: src.h }.intersect(&Rect { x: 0, y: 0, w: width, h: height })
    }

    fn remove_image(&mut self, id: u32) {
        self.remove_shown(|shown| shown.image == id);
        if let Some(old) = self.images.remove(&id) {
            self.used -= old.rgba.len();
        }
    }

    fn remove_shown(&mut self, mut which: impl FnMut(&Place) -> bool) {
        let mut gone = Vec::new();
        self.shown.retain(|shown| {
            let remove = which(&shown.place);
            if remove {
                gone.push(shown.place);
            }
            !remove
        });
        for place in gone {
            // The image is still stored here: placements go before their image.
            if let Some(rect) = self.rect_of(&place) {
                self.mark(rect);
            }
        }
    }

    /// Drops the oldest images until the stored bytes fit the budget: first
    /// images nothing shows, then shown ones. `keep` is never dropped.
    fn evict(&mut self, keep: u32) {
        while self.used > self.budget {
            let oldest = |shown_too: bool| {
                self.images
                    .iter()
                    .filter(|(id, _)| **id != keep)
                    .filter(|(id, _)| shown_too || !self.shown.iter().any(|shown| shown.place.image == **id))
                    .min_by_key(|(_, image)| image.seq)
                    .map(|(id, _)| *id)
            };
            let Some(id) = oldest(false).or_else(|| oldest(true)) else { return };
            self.remove_image(id);
        }
    }

    fn mark(&mut self, rect: Rect) {
        add_rect(&mut self.dirty, rect);
    }

    /// Redraws one area of the frame from the placements, bottom to top,
    /// starting at the highest opaque one that covers all of it.
    fn compose(&mut self, rect: Rect) {
        let (width, _) = self.size();
        let width = width as usize;
        let rects: Vec<Option<Rect>> = self.shown.iter().map(|shown| self.rect_of(&shown.place)).collect();
        let start = (0..self.shown.len()).rev().find(|&i| {
            let image = &self.images[&self.shown[i].place.image];
            image.opaque && rects[i].is_some_and(|shown| shown.contains(&rect))
        });
        if start.is_none() {
            for y in rect.y..rect.bottom() {
                let from = (y as usize * width + rect.x as usize) * 4;
                self.frame[from..from + rect.w as usize * 4].fill(0);
            }
        }
        let from = start.unwrap_or(0);
        for (shown, origin) in self.shown[from..].iter().zip(&rects[from..]) {
            let Some(origin) = *origin else { continue };
            let Some(area) = origin.intersect(&rect) else { continue };
            let place = &shown.place;
            let image = &self.images[&place.image];
            let src = place.src.unwrap_or_default();
            for y in area.y..area.bottom() {
                let src_y = (src.y + y - origin.y) as usize;
                let src_x = (src.x + area.x - origin.x) as usize;
                let from = (src_y * image.width as usize + src_x) * 4;
                let source = &image.rgba[from..from + area.w as usize * 4];
                let to = (y as usize * width + area.x as usize) * 4;
                let target = &mut self.frame[to..to + area.w as usize * 4];
                if image.opaque {
                    target.copy_from_slice(source);
                } else {
                    blend(target, source);
                }
            }
        }
    }
}

/// Adds a rectangle to a list. Overlapping ones merge; past MAX_RECTS the
/// pair that grows least when merged is merged.
fn add_rect(list: &mut Vec<Rect>, rect: Rect) {
    let mut rect = rect;
    while let Some(at) = list.iter().position(|other| other.intersect(&rect).is_some()) {
        rect = rect.union(&list.swap_remove(at));
    }
    list.push(rect);
    while list.len() > MAX_RECTS {
        let mut best = (0, 1, u64::MAX);
        for i in 0..list.len() {
            for j in i + 1..list.len() {
                // Rectangles in the list never overlap, so this cannot underflow.
                let growth = list[i].union(&list[j]).area() - list[i].area() - list[j].area();
                if growth < best.2 {
                    best = (i, j, growth);
                }
            }
        }
        let merged = list.swap_remove(best.1);
        let into = list.swap_remove(best.0);
        add_rect(list, into.union(&merged));
    }
}

/// Non-premultiplied `source` over `target`, one row.
fn blend(target: &mut [u8], source: &[u8]) {
    for (dst, src) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
        let sa = u32::from(src[3]);
        if sa == 255 {
            dst.copy_from_slice(src);
            continue;
        }
        if sa == 0 {
            continue;
        }
        let below = u32::from(dst[3]) * (255 - sa) / 255;
        let alpha = sa + below;
        for c in 0..3 {
            dst[c] = ((u32::from(src[c]) * sa + u32::from(dst[c]) * below + alpha / 2) / alpha) as u8;
        }
        dst[3] = alpha as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    /// 4 x 2 cells of 10 x 10 pixels.
    fn grid() -> Grid {
        Grid { cols: 4, rows: 2, width: 40, height: 20 }
    }

    fn solid(width: u32, height: u32, color: [u8; 4]) -> Image {
        Image { width, height, rgba: color.repeat((width * height) as usize) }
    }

    fn at(image: u32, row: u32, col: u32, dx: u32, dy: u32, z: i32) -> Place {
        Place { image, id: 1, row, col, dx, dy, src: None, z }
    }

    /// The color at a pixel after the next update, from a mirror of the frame.
    struct Mirror {
        width: u32,
        pixels: Vec<u8>,
    }

    impl Mirror {
        fn new(canvas: &Canvas) -> Self {
            let (width, height) = canvas.size();
            Self { width, pixels: vec![0; (width * height * 4) as usize] }
        }

        fn apply(&mut self, canvas: &mut Canvas) -> Vec<Rect> {
            let updates = canvas.take_dirty();
            let rects = updates.iter().map(|(rect, _)| *rect).collect();
            for (rect, data) in updates {
                assert_eq!(data.len(), (rect.w * rect.h * 4) as usize);
                for row in 0..rect.h {
                    let from = (row * rect.w * 4) as usize;
                    let to = (((rect.y + row) * self.width + rect.x) * 4) as usize;
                    self.pixels[to..to + (rect.w * 4) as usize].copy_from_slice(&data[from..from + (rect.w * 4) as usize]);
                }
            }
            rects
        }

        fn color(&self, x: u32, y: u32) -> [u8; 4] {
            let at = ((y * self.width + x) * 4) as usize;
            self.pixels[at..at + 4].try_into().unwrap()
        }
    }

    #[test]
    fn a_placement_lands_at_its_cell_plus_offset() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        assert_eq!(canvas.size(), (40, 20));
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(5, solid(2, 2, RED));
        assert!(canvas.place(at(5, 1, 2, 3, 4, 0)));
        assert!(canvas.is_dirty());
        assert_eq!(mirror.apply(&mut canvas), vec![Rect { x: 23, y: 14, w: 2, h: 2 }]);
        assert_eq!(mirror.color(23, 14), RED);
        assert_eq!(mirror.color(24, 15), RED);
        assert!(!canvas.is_dirty());
    }

    #[test]
    fn a_higher_z_covers_a_lower_one_in_any_order() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(4, 4, BLUE));
        canvas.place(at(1, 0, 0, 0, 0, 1));
        canvas.transmit(2, solid(4, 4, RED));
        canvas.place(at(2, 0, 0, 0, 0, 0));
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(1, 1), BLUE);
    }

    #[test]
    fn among_equal_z_the_newer_placement_is_on_top() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(4, 4, BLUE));
        canvas.place(at(1, 0, 0, 0, 0, 0));
        canvas.transmit(2, solid(4, 4, RED));
        canvas.place(at(2, 0, 0, 0, 0, 0));
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(1, 1), RED);
    }

    #[test]
    fn sending_an_id_again_removes_its_placement_and_shows_what_was_below() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(40, 20, GREEN));
        canvas.place(at(1, 0, 0, 0, 0, 0));
        canvas.transmit(7, solid(2, 2, RED));
        canvas.place(at(7, 0, 1, 0, 0, 1));
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(10, 0), RED);
        canvas.transmit(7, solid(2, 2, BLUE));
        assert_eq!(mirror.apply(&mut canvas), vec![Rect { x: 10, y: 0, w: 2, h: 2 }]);
        assert_eq!(mirror.color(10, 0), GREEN);
    }

    #[test]
    fn a_placement_with_the_same_id_moves() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(3, solid(2, 2, RED));
        canvas.place(at(3, 0, 0, 0, 0, 0));
        mirror.apply(&mut canvas);
        canvas.place(at(3, 1, 3, 0, 0, 0));
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(0, 0), CLEAR);
        assert_eq!(mirror.color(30, 10), RED);
    }

    #[test]
    fn deleting_everything_clears_the_frame() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(40, 20, GREEN));
        canvas.place(at(1, 0, 0, 0, 0, 0));
        mirror.apply(&mut canvas);
        canvas.delete(None, None, true);
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(39, 19), CLEAR);
        assert!(!canvas.place(at(1, 0, 0, 0, 0, 0)), "the image data is gone too");
    }

    #[test]
    fn deleting_placements_keeps_the_image_unless_it_is_freed() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(4, solid(2, 2, RED));
        canvas.place(at(4, 0, 0, 0, 0, 0));
        canvas.delete(Some(4), None, false);
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(0, 0), CLEAR);
        assert!(canvas.place(at(4, 0, 0, 0, 0, 0)));
        canvas.delete(Some(4), None, true);
        assert!(!canvas.place(at(4, 0, 0, 0, 0, 0)));
    }

    #[test]
    fn a_translucent_image_blends_over_the_one_below() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(2, 2, BLUE));
        canvas.place(at(1, 0, 0, 0, 0, 0));
        canvas.transmit(2, solid(2, 2, [255, 0, 0, 128]));
        canvas.place(at(2, 0, 0, 0, 0, 1));
        mirror.apply(&mut canvas);
        let [r, g, b, a] = mirror.color(0, 0);
        assert!((127..=129).contains(&r) && g == 0 && (126..=128).contains(&b) && a == 255, "{r} {g} {b} {a}");
    }

    #[test]
    fn a_source_rectangle_shows_part_of_the_image() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        let mut image = solid(4, 1, RED);
        image.rgba[8..12].copy_from_slice(&GREEN);
        canvas.transmit(1, image);
        let src = Some(Rect { x: 2, y: 0, w: 2, h: 1 });
        canvas.place(Place { src, ..at(1, 0, 0, 0, 0, 0) });
        assert_eq!(mirror.apply(&mut canvas), vec![Rect { x: 0, y: 0, w: 2, h: 1 }]);
        assert_eq!(mirror.color(0, 0), GREEN);
        assert_eq!(mirror.color(1, 0), RED);
    }

    #[test]
    fn a_placement_is_cut_at_the_frame_edge() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(30, 30, RED));
        canvas.place(at(1, 1, 3, 5, 5, 0));
        assert_eq!(mirror.apply(&mut canvas), vec![Rect { x: 35, y: 15, w: 5, h: 5 }]);
        assert_eq!(mirror.color(39, 19), RED);
    }

    #[test]
    fn many_small_changes_stay_within_the_rectangle_limit() {
        let grid = Grid { cols: 100, rows: 100, width: 1000, height: 1000 };
        let mut canvas = Canvas::new(grid, 1 << 24);
        let mut mirror = Mirror::new(&canvas);
        for i in 0..50 {
            canvas.transmit(i + 1, solid(2, 2, RED));
            canvas.place(at(i + 1, i * 2, i * 2, 0, 0, 0));
        }
        let rects = mirror.apply(&mut canvas);
        assert!(rects.len() <= MAX_RECTS, "{} rects", rects.len());
        for i in 0..50 {
            assert_eq!(mirror.color(i * 20, i * 20), RED, "placement {i}");
        }
    }

    #[test]
    fn over_budget_images_without_placements_go_first() {
        // Room for two 10 x 10 images.
        let mut canvas = Canvas::new(grid(), 2 * 400);
        let mut mirror = Mirror::new(&canvas);
        canvas.transmit(1, solid(10, 10, RED));
        canvas.place(at(1, 0, 0, 0, 0, 0));
        canvas.transmit(2, solid(10, 10, GREEN));
        canvas.transmit(3, solid(10, 10, BLUE));
        assert!(!canvas.place(at(2, 0, 1, 0, 0, 0)), "the unplaced image was dropped");
        assert!(canvas.place(at(3, 0, 2, 0, 0, 0)));
        mirror.apply(&mut canvas);
        assert_eq!(mirror.color(0, 0), RED);
        assert_eq!(mirror.color(20, 0), BLUE);
    }

    #[test]
    fn a_resize_to_the_same_size_changes_nothing() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        canvas.transmit(1, solid(2, 2, RED));
        canvas.place(at(1, 0, 0, 0, 0, 0));
        canvas.take_dirty();
        canvas.resize(grid());
        assert!(!canvas.is_dirty());
    }

    #[test]
    fn a_resize_moves_placements_to_their_cells_and_redraws_everything() {
        let mut canvas = Canvas::new(grid(), 1 << 20);
        canvas.transmit(1, solid(2, 2, RED));
        canvas.place(at(1, 1, 1, 0, 0, 0));
        canvas.take_dirty();
        canvas.resize(Grid { cols: 4, rows: 2, width: 80, height: 40 });
        assert_eq!(canvas.size(), (80, 40));
        let mut mirror = Mirror::new(&canvas);
        assert_eq!(mirror.apply(&mut canvas), vec![Rect { x: 0, y: 0, w: 80, h: 40 }]);
        assert_eq!(mirror.color(20, 20), RED);
        assert_eq!(mirror.color(10, 10), CLEAR);
    }
}
