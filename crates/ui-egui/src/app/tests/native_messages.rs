//! Real native callbacks in the owned CPU renderer; no OS input or clipboard writes.
use super::native_bridge::Bridge;
use crate::native_dialog::message::{self, Answer, Button, Kind, Message, Role};
#[test]
fn stock_alert_buttons_and_sizes_match_original_java_scenes() {
    let original: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/dialogs/original-object-alerts.json"
    ))
    .unwrap();
    for case in original.as_array().unwrap() {
        let key = case["kind"].as_str().unwrap();
        let (kind, header, content, custom) = match key {
            "drop" => (
                Kind::Confirmation,
                None,
                "确定要删除选中的 2 张表、1 个视图吗？此操作不可恢复！",
                Some("确认删除"),
            ),
            "clear" => (
                Kind::Confirmation,
                None,
                "确定要清空选中的 2 张表的数据吗？\n此操作将删除所有数据（DELETE FROM），不可恢复！",
                Some("确认清空"),
            ),
            "created" => (Kind::Information, None, "表 orders 创建成功", None),
            "rename-error" => (Kind::Error, None, "对象名称已存在", None),
            _ => (
                Kind::Warning,
                Some("字段设置不完整，请检查以下问题"),
                "字段 1: 类型不能为空\n字段 2: 需要指定长度",
                None,
            ),
        };
        let buttons = if let Some(label) = custom {
            vec![
                Button {
                    label,
                    role: Role::Other,
                },
                Button {
                    label: "取消",
                    role: Role::Cancel,
                },
            ]
        } else {
            vec![Button {
                label: "确定",
                role: Role::Ok,
            }]
        };
        let ctx = egui::Context::default();
        let bridge = Bridge::new(&ctx);
        let mut icons = crate::icons::Icons::default();
        let mut focus = true;
        let mut observed = None;
        bridge.frame(&ctx, Vec::new(), |ctx| {
            assert!(
                message::show(
                    ctx,
                    Message {
                        id: egui::ViewportId::from_hash_of(("stock-alert-test", key)),
                        owner: None,
                        title: key,
                        header,
                        content,
                        kind,
                        buttons: &buttons,
                        active: false
                    },
                    &mut focus,
                    &mut icons,
                    |ui, rects| {
                        observed = Some((ui.max_rect().size(), rects.to_vec()));
                    }
                )
                .is_none()
            );
        });
        let (size, rects) = observed.unwrap();
        assert_eq!(
            size,
            egui::vec2(
                case["width"].as_f64().unwrap() as f32,
                case["height"].as_f64().unwrap() as f32
            ),
            "{key}"
        );
        let expected: Vec<_> = case["controls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|control| control["class"] == "Button")
            .collect();
        assert_eq!(expected.len(), rects.len());
        for (rect, source) in rects.iter().zip(expected) {
            for (actual, key) in [
                (rect.min.x, "x"),
                (rect.min.y, "y"),
                (rect.width(), "width"),
                (rect.height(), "height"),
            ] {
                assert!(
                    (f64::from(actual) - source[key].as_f64().unwrap()).abs() < 0.01,
                    "{key}"
                );
            }
        }
    }
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}
#[test]
fn custom_delete_has_no_enter_default_esc_cancels_and_click_selects_the_actual_button() {
    for click in [false, true] {
        let ctx = egui::Context::default();
        let bridge = Bridge::new(&ctx);
        let mut icons = crate::icons::Icons::default();
        let mut focus = true;
        let mut answer = None;
        let mut bounds = None;
        let buttons = [
            Button {
                label: "确认删除",
                role: Role::Other,
            },
            Button {
                label: "取消",
                role: Role::Cancel,
            },
        ];
        let mut draw = |ctx: &egui::Context| {
            if answer.is_none() {
                answer = message::show(
                    ctx,
                    Message {
                        id: egui::ViewportId::from_hash_of("delete-alert-test"),
                        owner: None,
                        title: "确认删除",
                        header: None,
                        content: "确定要删除选中的 2 张表、1 个视图吗？此操作不可恢复！",
                        kind: Kind::Confirmation,
                        buttons: &buttons,
                        active: true,
                    },
                    &mut focus,
                    &mut icons,
                    |ui, rects| {
                        bounds = Some(rects.to_vec());
                        ui.memory_mut(|memory| memory.surrender_focus(ui.id().with("确认删除")));
                    },
                );
            }
        };
        bridge.frame(&ctx, Vec::new(), &mut draw);
        bridge.frame(&ctx, vec![key(egui::Key::Enter)], &mut draw);
        assert!(answer.is_none(), "OTHER is not an OpenJFX default button");
        let first = bounds.as_ref().unwrap()[0].center();
        let mut draw = |ctx: &egui::Context| {
            if answer.is_none() {
                answer = message::show(
                    ctx,
                    Message {
                        id: egui::ViewportId::from_hash_of("delete-alert-test"),
                        owner: None,
                        title: "确认删除",
                        header: None,
                        content: "确定要删除选中的 2 张表、1 个视图吗？此操作不可恢复！",
                        kind: Kind::Confirmation,
                        buttons: &buttons,
                        active: true,
                    },
                    &mut focus,
                    &mut icons,
                    |_, _| {},
                );
            }
        };
        if click {
            bridge.click(&ctx, first, &mut draw);
        } else {
            bridge.frame(&ctx, vec![key(egui::Key::Escape)], &mut draw);
        }
        assert!(matches!(answer,Some(Answer::Button(index)) if index==if click{0}else{1}));
    }
}
#[test]
fn ordinary_information_enter_accepts_and_yes_no_escape_uses_no() {
    for yes_no in [false, true] {
        let ctx = egui::Context::default();
        let bridge = Bridge::new(&ctx);
        let mut icons = crate::icons::Icons::default();
        let mut focus = true;
        let mut answer = None;
        let buttons = if yes_no {
            vec![
                Button {
                    label: "是",
                    role: Role::Yes,
                },
                Button {
                    label: "否",
                    role: Role::No,
                },
            ]
        } else {
            vec![Button {
                label: "确定",
                role: Role::Ok,
            }]
        };
        let mut draw = |ctx: &egui::Context| {
            if answer.is_none() {
                answer = message::show(
                    ctx,
                    Message {
                        id: egui::ViewportId::from_hash_of("stock-key-test"),
                        owner: None,
                        title: "提示",
                        header: None,
                        content: "内容",
                        kind: Kind::Information,
                        buttons: &buttons,
                        active: true,
                    },
                    &mut focus,
                    &mut icons,
                    |_, _| {},
                );
            }
        };
        bridge.frame(&ctx, Vec::new(), &mut draw);
        bridge.frame(
            &ctx,
            vec![key(if yes_no {
                egui::Key::Escape
            } else {
                egui::Key::Enter
            })],
            &mut draw,
        );
        assert!(matches!(answer,Some(Answer::Button(index)) if index==usize::from(yes_no)));
    }
}
#[test]
fn focused_custom_confirmation_accepts_enter_without_becoming_a_default_button() {
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut icons = crate::icons::Icons::default();
    let mut focus = true;
    let mut answer = None;
    let buttons = [
        Button {
            label: "确认清空",
            role: Role::Other,
        },
        Button {
            label: "取消",
            role: Role::Cancel,
        },
    ];
    let mut draw = |ctx: &egui::Context| {
        if answer.is_none() {
            answer = message::show(
                ctx,
                Message {
                    id: egui::ViewportId::from_hash_of("focused-other-test"),
                    owner: None,
                    title: "清空确认",
                    header: None,
                    content: "确定要清空选中的 2 张表的数据吗？",
                    kind: Kind::Confirmation,
                    buttons: &buttons,
                    active: true,
                },
                &mut focus,
                &mut icons,
                |_, _| {},
            );
        }
    };
    bridge.frame(&ctx, Vec::new(), &mut draw);
    bridge.frame(&ctx, vec![key(egui::Key::Enter)], &mut draw);
    assert!(matches!(answer, Some(Answer::Button(0))));
}

#[test]
fn window_close_cancels_a_confirmation_and_dismisses_a_single_ok_alert() {
    for confirmation in [false, true] {
        let ctx = egui::Context::default();
        let bridge = Bridge::new(&ctx);
        let mut icons = crate::icons::Icons::default();
        let mut focus = true;
        let mut answer = None;
        let id = egui::ViewportId::from_hash_of(("close-alert-test", confirmation));
        let buttons = if confirmation {
            vec![
                Button {
                    label: "是",
                    role: Role::Yes,
                },
                Button {
                    label: "否",
                    role: Role::No,
                },
            ]
        } else {
            vec![Button {
                label: "确定",
                role: Role::Ok,
            }]
        };
        let mut draw = |ctx: &egui::Context| {
            answer = message::show(
                ctx,
                Message {
                    id,
                    owner: None,
                    title: "测试",
                    header: None,
                    content: "内容",
                    kind: Kind::Information,
                    buttons: &buttons,
                    active: false,
                },
                &mut focus,
                &mut icons,
                |_, _| {},
            );
        };
        bridge.frame(&ctx, Vec::new(), &mut draw);
        bridge.close_viewport(&ctx, id, &mut draw);
        assert!(if confirmation {
            matches!(answer, Some(Answer::Button(1)))
        } else {
            matches!(answer, Some(Answer::Closed))
        });
    }
}
