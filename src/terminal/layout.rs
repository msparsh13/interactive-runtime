use crate::terminal::renderer::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Panes next to each other (vertical divider).
    SideBySide,
    /// Panes above each other (horizontal divider).
    Stacked,
}

pub enum Node {
    Leaf(usize), // pane id
    Split {
        dir: Dir,
        ratio: f32, // share of the first child
        a: Box<Node>,
        b: Box<Node>,
    },
}

pub struct Placed {
    pub id: usize,
    pub rect: Rect, // includes the scrollbar column
}

pub struct Divider {
    pub rect: Rect,      // grab area
    pub path: Vec<bool>, // route to the Split node (false = a, true = b)
    pub dir: Dir,
    pub area: Rect, // area the whole split occupies
}

/// Returns (first child, grab handle, second child).
fn split_rect(area: Rect, dir: Dir, ratio: f32) -> (Rect, Rect, Rect) {
    match dir {
        Dir::SideBySide => {
            let w = area.width;
            let lo = 2.min(w);
            let hi = w.saturating_sub(2).max(lo);
            let wa = ((w as f32 * ratio).round() as u16).clamp(lo, hi);
            let a = Rect { width: wa, ..area };
            // The handle is the last column of `a` (its scrollbar).
            let handle = Rect {
                x: (area.x + wa).saturating_sub(1),
                width: 1,
                ..area
            };
            let b = Rect {
                x: area.x + wa,
                width: w - wa,
                ..area
            };
            (a, handle, b)
        }
        Dir::Stacked => {
            let h = area.height;
            let lo = 1.min(h);
            let hi = h.saturating_sub(2).max(lo);
            let ha = ((h as f32 * ratio).round() as u16).clamp(lo, hi);
            let div = 1.min(h - ha);
            let a = Rect { height: ha, ..area };
            let handle = Rect {
                y: area.y + ha,
                height: div,
                ..area
            };
            let b = Rect {
                y: area.y + ha + div,
                height: h - ha - div,
                ..area
            };
            (a, handle, b)
        }
    }
}

impl Node {
    pub fn layout(
        &self,
        area: Rect,
        path: &mut Vec<bool>,
        panes: &mut Vec<Placed>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            Node::Leaf(id) => panes.push(Placed {
                id: *id,
                rect: area,
            }),
            Node::Split { dir, ratio, a, b } => {
                let (ra, handle, rb) = split_rect(area, *dir, *ratio);
                dividers.push(Divider {
                    rect: handle,
                    path: path.clone(),
                    dir: *dir,
                    area,
                });
                path.push(false);
                a.layout(ra, path, panes, dividers);
                path.pop();
                path.push(true);
                b.layout(rb, path, panes, dividers);
                path.pop();
            }
        }
    }

    /// Replace leaf `target` with a split of (target, new_id).
    pub fn split_leaf(&mut self, target: usize, dir: Dir, new_id: usize) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let old = Node::Leaf(*id);
                *self = Node::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(old),
                    b: Box::new(Node::Leaf(new_id)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => {
                a.split_leaf(target, dir, new_id) || b.split_leaf(target, dir, new_id)
            }
        }
    }

    /// Remove a leaf; its sibling takes the parent's place.
    pub fn remove(self, id: usize) -> Option<Node> {
        match self {
            Node::Leaf(i) if i == id => None,
            Node::Leaf(_) => Some(self),
            Node::Split { dir, ratio, a, b } => match ((*a).remove(id), (*b).remove(id)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(n), None) | (None, Some(n)) => Some(n),
                (None, None) => None,
            },
        }
    }

    pub fn at_mut(&mut self, path: &[bool]) -> Option<&mut Node> {
        match path.split_first() {
            None => Some(self),
            Some((&go_b, rest)) => match self {
                Node::Split { a, b, .. } => {
                    if go_b {
                        b.at_mut(rest)
                    } else {
                        a.at_mut(rest)
                    }
                }
                Node::Leaf(_) => None,
            },
        }
    }
}
