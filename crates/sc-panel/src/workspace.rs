//! Two sides with tabs.

use sc_index::SortSpec;
use sc_vfs::VfsPath;

use crate::panel::{LoadRequest, Panel};

/// Left or right half of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideId {
    /// Left side.
    Left,
    /// Right side.
    Right,
}

impl SideId {
    /// The other side.
    pub fn other(self) -> SideId {
        match self {
            SideId::Left => SideId::Right,
            SideId::Right => SideId::Left,
        }
    }
}

/// The tabs of one side; there is always at least one.
#[derive(Debug, Clone)]
pub struct Side {
    tabs: Vec<Panel>,
    active: usize,
}

impl Side {
    /// A side with one tab.
    pub fn new(panel: Panel) -> Self {
        Side { tabs: vec![panel], active: 0 }
    }

    /// The visible panel.
    pub fn panel(&self) -> &Panel {
        &self.tabs[self.active]
    }

    /// The visible panel, mutably.
    pub fn panel_mut(&mut self) -> &mut Panel {
        &mut self.tabs[self.active]
    }

    /// All tabs.
    pub fn tabs(&self) -> &[Panel] {
        &self.tabs
    }

    /// Index of the visible tab.
    pub fn active_tab(&self) -> usize {
        self.active
    }

    /// Opens a new tab on the same location, right after the current one,
    /// and returns the request to fill it.
    pub fn new_tab(&mut self) -> LoadRequest {
        let current = self.panel();
        let mut tab = Panel::new(current.location().clone(), current.sort(), current.show_hidden());
        let request = tab.reload();
        self.active += 1;
        self.tabs.insert(self.active, tab);
        request
    }

    /// Closes the visible tab (never the last one). Returns whether it closed.
    pub fn close_tab(&mut self) -> bool {
        if self.tabs.len() == 1 {
            return false;
        }
        self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len() - 1);
        true
    }

    /// Switches tab by `delta`, wrapping around.
    pub fn cycle_tab(&mut self, delta: isize) {
        let count = self.tabs.len() as isize;
        self.active = (self.active as isize + delta).rem_euclid(count) as usize;
    }

    /// Selects a tab by index (ignored if out of range).
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }
}

/// Both sides of the window.
#[derive(Debug, Clone)]
pub struct Workspace {
    left: Side,
    right: Side,
    active: SideId,
}

impl Workspace {
    /// A workspace with one tab per side. Returns it together with the two
    /// initial load requests.
    pub fn new(
        left: VfsPath,
        right: VfsPath,
        sort: SortSpec,
        show_hidden: bool,
    ) -> (Self, [(SideId, LoadRequest); 2]) {
        let mut left = Panel::new(left, sort, show_hidden);
        let mut right = Panel::new(right, sort, show_hidden);
        let requests = [(SideId::Left, left.reload()), (SideId::Right, right.reload())];
        (
            Workspace { left: Side::new(left), right: Side::new(right), active: SideId::Left },
            requests,
        )
    }

    /// Which side has the focus.
    pub fn active_side(&self) -> SideId {
        self.active
    }

    /// Gives the focus to `side`.
    pub fn set_active(&mut self, side: SideId) {
        self.active = side;
    }

    /// Moves the focus to the other side.
    pub fn switch_side(&mut self) {
        self.active = self.active.other();
    }

    /// A side.
    pub fn side(&self, id: SideId) -> &Side {
        match id {
            SideId::Left => &self.left,
            SideId::Right => &self.right,
        }
    }

    /// A side, mutably.
    pub fn side_mut(&mut self, id: SideId) -> &mut Side {
        match id {
            SideId::Left => &mut self.left,
            SideId::Right => &mut self.right,
        }
    }

    /// The focused panel.
    pub fn active(&self) -> &Panel {
        self.side(self.active).panel()
    }

    /// The focused panel, mutably.
    pub fn active_mut(&mut self) -> &mut Panel {
        let id = self.active;
        self.side_mut(id).panel_mut()
    }

    /// The panel on the other side (the usual copy/move destination).
    pub fn inactive(&self) -> &Panel {
        self.side(self.active.other()).panel()
    }

    /// The panel on the other side, mutably.
    pub fn inactive_mut(&mut self) -> &mut Panel {
        let id = self.active.other();
        self.side_mut(id).panel_mut()
    }

    /// Exchanges the two sides (tabs included); the focus stays on the
    /// same side of the screen.
    pub fn swap_sides(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }

    /// Shows the focused panel's location in the other panel too.
    pub fn same_location_in_other(&mut self) -> LoadRequest {
        let location = self.active().location().clone();
        self.inactive_mut().navigate(location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> VfsPath {
        VfsPath::parse(path).unwrap()
    }

    #[test]
    fn sides_and_focus() {
        let (mut ws, requests) = Workspace::new(p("/a"), p("/b"), SortSpec::default(), false);
        assert_eq!(requests[0].1.path, p("/a"));
        assert_eq!(ws.active().location(), &p("/a"));
        ws.switch_side();
        assert_eq!(ws.active_side(), SideId::Right);
        assert_eq!(ws.inactive().location(), &p("/a"));
        ws.swap_sides();
        assert_eq!(
            ws.active().location(),
            &p("/a"),
            "focus stays on the right, which now shows /a"
        );
        let request = ws.same_location_in_other();
        assert_eq!(request.path, p("/a"));
    }

    #[test]
    fn tabs() {
        let (mut ws, _) = Workspace::new(p("/a"), p("/b"), SortSpec::default(), false);
        let side = ws.side_mut(SideId::Left);
        assert!(!side.close_tab(), "the last tab stays");
        let request = side.new_tab();
        assert_eq!(request.path, p("/a"));
        assert_eq!(side.tabs().len(), 2);
        assert_eq!(side.active_tab(), 1);
        side.cycle_tab(1);
        assert_eq!(side.active_tab(), 0);
        side.cycle_tab(-1);
        assert_eq!(side.active_tab(), 1);
        assert!(side.close_tab());
        assert_eq!(side.active_tab(), 0);
        side.select_tab(5);
        assert_eq!(side.active_tab(), 0);
    }
}
