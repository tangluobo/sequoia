//! Original ColorTransposeGamePane stack puzzle. A move stores only two tube indices.
use std::collections::VecDeque;
pub struct Game {
    pub dimension: usize,
    pub columns: Vec<Vec<u8>>,
    pub selected: Option<usize>,
    pub moves: u64,
    pub status: String,
    pub victory_style: bool,
    history: VecDeque<(u8, u8)>,
}
impl Default for Game {
    fn default() -> Self {
        let mut game = Self {
            dimension: 3,
            columns: vec![],
            selected: None,
            moves: 0,
            status: String::new(),
            victory_style: false,
            history: VecDeque::new(),
        };
        game.start("3");
        game
    }
}
impl Game {
    pub fn start(&mut self, input: &str) {
        let n = input
            .trim_matches(|c| c <= '\u{20}')
            .parse::<i32>()
            .unwrap_or(self.dimension as i32)
            .clamp(2, 20) as usize;
        self.dimension = n;
        self.columns = (0..n)
            .map(|i| {
                if i + 1 == n {
                    Vec::with_capacity(n)
                } else {
                    (0..n as u8).collect()
                }
            })
            .collect();
        self.selected = None;
        self.moves = 0;
        self.history.clear();
        self.history.shrink_to(128);
        self.victory_style = false;
        self.status =
            "规则：点击一个容器取其顶部珠子，再点击另一容器放至顶部。让每个容器内同色即胜利。"
                .into();
    }
    pub fn click(&mut self, column: usize) {
        let Some(target) = self.columns.get(column) else {
            return;
        };
        let Some(from) = self.selected else {
            if target.is_empty() {
                self.status = "该容器为空，无可取珠子。".into();
            } else {
                self.selected = Some(column);
                self.status = format!("已取第 {} 个容器顶部珠子，点击另一容器放置。", column + 1);
            }
            return;
        };
        if from == column {
            self.selected = None;
            self.status = "已取消选择。".into();
            return;
        }
        if target.len() >= self.dimension {
            self.status = format!("该容器已满（容量 {}），无法放入。", self.dimension);
            return;
        }
        let Some(bead) = self.columns[from].pop() else {
            self.selected = None;
            return;
        };
        self.columns[column].push(bead);
        self.selected = None;
        self.moves = self.moves.saturating_add(1);
        if self.history.len() == 100_000 {
            self.history.pop_front();
        }
        self.history.push_back((from as u8, column as u8));
        if self.won() {
            self.victory_style = true;
            self.status = format!(
                "胜利！共用 {} 步完成转置。点击「重置」再玩一次。",
                self.moves
            );
        } else {
            self.status = format!("已移动 {} 步。", self.moves);
        }
    }
    pub fn undo(&mut self) {
        let Some((from, to)) = self.history.pop_back() else {
            self.status = "无可撤销的操作。".into();
            return;
        };
        if let Some(bead) = self.columns[to as usize].pop() {
            self.columns[from as usize].push(bead);
        }
        self.moves = self.moves.saturating_sub(1);
        self.selected = None;
        self.victory_style = false;
        self.status = format!("已撤销，当前 {} 步。", self.moves);
    }
    pub fn won(&self) -> bool {
        self.columns
            .iter()
            .all(|c| c.first().is_none_or(|first| c.iter().all(|v| v == first)))
    }
}
pub fn palette(index: usize) -> [u8; 3] {
    let value = (index % 20) * 47 % 215;
    [
        (value % 6) as u8 * 51,
        (value / 6 % 6) as u8 * 51,
        (value / 36) as u8 * 51,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(game: &Game) -> serde_json::Value {
        serde_json::json!({"n":game.dimension,"columns":game.columns,"moveCount":game.moves,"selectedCol":game.selected.map_or(-1,|i|i as i32),"status":game.status,"style":if game.victory_style{"-fx-font-size: 14px; -fx-text-fill: #07c160; -fx-font-weight: bold;"}else{"-fx-font-size: 13px; -fx-text-fill: #666;"}})
    }
    #[test]
    fn original_java_interaction_goldens() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/color-transpose/cases.json"))
                .unwrap();
        let mut game = Game::default();
        for case in cases.as_array().unwrap() {
            game.start(case["dimension"].as_str().unwrap());
            assert_eq!(snapshot(&game), case["start"]);
            for (column, expected) in [0, 0, 0, 1, 2, 0, 2, 1, 0]
                .into_iter()
                .zip(case["states"].as_array().unwrap())
            {
                game.click(column % game.dimension);
                assert_eq!(snapshot(&game), *expected);
            }
            game.undo();
            assert_eq!(snapshot(&game), case["undo"]);
        }
    }
    #[test]
    fn actual_win_and_undo_keep_all_beads() {
        let mut game = Game::default();
        game.start("2");
        game.click(0);
        game.click(1);
        assert!(game.won());
        assert!(game.victory_style);
        assert_eq!(game.columns, vec![vec![0], vec![1]]);
        game.undo();
        assert!(!game.won());
        assert_eq!(game.moves, 0);
        game.click(0);
        game.undo();
        assert_eq!(game.selected, Some(0));
        assert_eq!(game.status, "无可撤销的操作。");
    }
}
