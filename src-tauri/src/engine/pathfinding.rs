use std::collections::{BinaryHeap, HashMap};
use std::cmp::Ordering;

use crate::engine::state::{TileType, Tile};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PathNode {
    pub x: i32,
    pub y: i32,
    pub g: i32,
    pub h: i32,
}

impl Ord for PathNode {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_f = self.g + self.h;
        let other_f = other.g + other.h;
        other_f.cmp(&self_f).then_with(|| other.g.cmp(&self.g))
    }
}

impl PartialOrd for PathNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn manhattan(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    (x1 - x2).abs() + (y1 - y2).abs()
}

fn is_passable(tile: &Tile) -> bool {
    matches!(tile.tile_type, TileType::Floor | TileType::Door | TileType::Stairs)
}

pub fn find_path(
    tiles: &[Vec<Tile>],
    start_x: i32,
    start_y: i32,
    goal_x: i32,
    goal_y: i32,
    occupied: &[(i32, i32)],
    max_cost: i32,
) -> Option<Vec<(i32, i32)>> {
    let height = tiles.len() as i32;
    if height == 0 { return None; }
    let width = tiles[0].len() as i32;

    if start_x == goal_x && start_y == goal_y {
        return Some(vec![(goal_x, goal_y)]);
    }

    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_scores: HashMap<(i32, i32), i32> = HashMap::new();

    let start_key = (start_x, start_y);
    open_set.push(PathNode { x: start_x, y: start_y, g: 0, h: manhattan(start_x, start_y, goal_x, goal_y) });
    g_scores.insert(start_key, 0);

    let directions = [(0, -1), (0, 1), (-1, 0), (1, 0)];

    while let Some(node) = open_set.pop() {
        let key = (node.x, node.y);
        let current_g = *g_scores.get(&key).unwrap_or(&i32::MAX);

        if node.x == goal_x && node.y == goal_y {
            let mut path = Vec::new();
            let mut current = (goal_x, goal_y);
            while let Some(&prev) = came_from.get(&current) {
                path.push(current);
                current = prev;
            }
            path.reverse();
            return Some(path);
        }

        if current_g > max_cost {
            continue;
        }

        for &(dx, dy) in &directions {
            let nx = node.x + dx;
            let ny = node.y + dy;

            if nx < 0 || ny < 0 || nx >= width || ny >= height {
                continue;
            }

            let tile = &tiles[ny as usize][nx as usize];
            if !is_passable(tile) {
                continue;
            }

            if occupied.contains(&(nx, ny)) && !(nx == goal_x && ny == goal_y) {
                continue;
            }

            let tentative_g = current_g + 1;
            if tentative_g > max_cost {
                continue;
            }

            let neighbor_key = (nx, ny);
            if tentative_g < *g_scores.get(&neighbor_key).unwrap_or(&i32::MAX) {
                g_scores.insert(neighbor_key, tentative_g);
                came_from.insert(neighbor_key, key);
                open_set.push(PathNode {
                    x: nx,
                    y: ny,
                    g: tentative_g,
                    h: manhattan(nx, ny, goal_x, goal_y),
                });
            }
        }
    }

    None
}
