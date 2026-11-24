use super::*;
use tomato_core::game_dungeons::{Account, Character, Dungeon, Extra, Weekday};

#[test]
fn large_board_draws_only_visible_rows_and_columns() {
    let mut board = Board::default();
    board.accounts.push(Account {
        id: "a".into(),
        name: "测试账号".into(),
        extra: Extra::new(),
    });
    for i in 0..1000 {
        board.characters.push(Character {
            id: format!("c{i}"),
            account_id: "a".into(),
            name: format!("角色{i}"),
            extra: Extra::new(),
        });
    }
    for i in 0..100 {
        board.dungeons.push(Dungeon {
            id: format!("d{i}"),
            name: format!("副本{i}"),
            reset_day: Weekday::Thursday,
            extra: Extra::new(),
        });
    }
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut fonts = egui::FontDefinitions::default();
    let family = fonts.families[&egui::FontFamily::Proportional].clone();
    fonts
        .families
        .insert(egui::FontFamily::Name("java-ui".into()), family);
    ctx.set_fonts(fonts);
    let mut shapes = 0;
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(800., 600.),
                )),
                ..Default::default()
            },
            |ui| {
                matrix(ui, &board, None, "", 739_939, true);
            },
        );
        shapes = output.shapes.len();
        output.textures_delta.clear();
    }
    assert!(shapes < 1500, "large board rendered {shapes} shapes");
}
