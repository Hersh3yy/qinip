//! Playlist row selection: a focused *anchor* row plus a multi-select set.
//!
//! The anchor is what single-row actions (Enter, waveform, transport) act on.
//! The set is what batch actions (Backspace/Delete) act on. Plain click sets
//! both; Shift+click extends a range from the anchor; Cmd/Ctrl+click toggles.

use std::collections::BTreeSet;

#[derive(Debug, Default, Clone)]
pub struct Selection {
    anchor: Option<usize>,
    set: BTreeSet<usize>,
}

impl Selection {
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    pub fn contains(&self, index: usize) -> bool {
        self.set.contains(&index)
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Plain click: this row becomes the only selected row and the anchor.
    pub fn select_only(&mut self, index: usize) {
        self.anchor = Some(index);
        self.set.clear();
        self.set.insert(index);
    }

    /// Cmd/Ctrl+click: toggle membership; the clicked row becomes the anchor.
    pub fn toggle(&mut self, index: usize) {
        if !self.set.remove(&index) {
            self.set.insert(index);
        }
        self.anchor = Some(index);
    }

    /// Shift+click: select the inclusive range between the anchor and this row.
    /// Without an anchor it behaves like a plain click.
    pub fn extend_to(&mut self, index: usize) {
        let Some(anchor) = self.anchor else {
            self.select_only(index);
            return;
        };
        let (lo, hi) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        self.set.extend(lo..=hi);
    }

    pub fn select_all(&mut self, len: usize) {
        self.set = (0..len).collect();
        if self.anchor.is_none() && len > 0 {
            self.anchor = Some(0);
        }
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.set.clear();
    }

    /// Selected indices, highest first — the safe order for removing rows
    /// from a `Vec` without invalidating the indices still to be removed.
    pub fn indices_desc(&self) -> Vec<usize> {
        self.set.iter().rev().copied().collect()
    }

    /// Re-point every stored index through `map` (e.g. after a sort), dropping
    /// rows that no longer exist.
    pub fn remap(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.set = self.set.iter().filter_map(|&i| map(i)).collect();
        self.anchor = self.anchor.and_then(&map);
    }

    /// Keep the selection consistent after row `removed` left the list:
    /// drop it, shift later rows down by one.
    pub fn after_remove(&mut self, removed: usize) {
        self.remap(|i| {
            if i == removed {
                None
            } else if i > removed {
                Some(i - 1)
            } else {
                Some(i)
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_click_selects_inclusive_range_either_direction() {
        let mut s = Selection::default();
        s.select_only(4);
        s.extend_to(1);
        assert_eq!(s.indices_desc(), vec![4, 3, 2, 1]);
        s.select_only(1);
        s.extend_to(3);
        assert_eq!(s.indices_desc(), vec![3, 2, 1]);
    }

    #[test]
    fn after_remove_shifts_later_indices_down() {
        let mut s = Selection::default();
        s.select_all(5);
        s.after_remove(2);
        assert_eq!(s.indices_desc(), vec![3, 2, 1, 0]);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut s = Selection::default();
        s.toggle(2);
        s.toggle(5);
        assert!(s.contains(2) && s.contains(5));
        s.toggle(2);
        assert!(!s.contains(2));
        assert_eq!(s.anchor(), Some(2));
    }
}
