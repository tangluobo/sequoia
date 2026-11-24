use super::*;
use anyhow::Context as _;
use std::time::{Duration, Instant};

struct Controls {
    context: egui::Context,
    time: f64,
}
mod mutations;
impl Controls {
    fn frame(&mut self, app: &mut TomatoApp, mut events: Vec<egui::Event>) -> egui::FullOutput {
        let modifiers = events
            .iter()
            .find_map(|event| match event {
                egui::Event::Key { modifiers, .. }
                | egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        self.time = self.time.max(self.context.input(|input| input.time)) + 0.03;
        app.logic(&self.context);
        let mut output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 900.),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        output
    }
    fn click(
        &mut self,
        app: &mut TomatoApp,
        text: &str,
        secondary: bool,
        menu: bool,
    ) -> anyhow::Result<()> {
        let frame = self.frame(app, vec![]);
        let mut positions = frame.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.job.text == text => Some(t.pos + t.galley.size() / 2.),
            _ => None,
        });
        let point = if menu {
            positions.next_back()
        } else {
            positions.next()
        }
        .with_context(|| format!("object control missing: {text}"))?;
        self.click_at(app, point, secondary, Default::default());
        Ok(())
    }
    fn click_at(
        &mut self,
        app: &mut TomatoApp,
        point: egui::Pos2,
        secondary: bool,
        modifiers: egui::Modifiers,
    ) {
        for pressed in [true, false] {
            self.frame(
                app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: if secondary {
                            egui::PointerButton::Secondary
                        } else {
                            egui::PointerButton::Primary
                        },
                        pressed,
                        modifiers,
                    },
                ],
            );
        }
    }
    fn wait(&mut self, app: &mut TomatoApp) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(25);
        while !app.pending.is_empty() {
            self.frame(app, vec![]);
            anyhow::ensure!(Instant::now() < deadline, "object job timeout");
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}
fn workbench(tab: &Tab) -> &crate::tabs::workbench::Workbench {
    let Panel::Sql { workbench, .. } = &tab.panel else {
        panic!("expected SQL objects tab")
    };
    workbench
}

#[test]
#[ignore = "requires owned MySQL fixture"]
fn mysql_native_objects_bulk_open_design_and_deferred_load() -> anyhow::Result<()> {
    bulk_open_fixture("MYSQL", "TOMATO_TEST_MYSQL_URL", "mysql", "root")
}

#[test]
#[ignore = "requires owned PostgreSQL fixture"]
fn postgres_native_objects_bulk_open_design_and_deferred_load() -> anyhow::Result<()> {
    bulk_open_fixture(
        "POSTGRESQL",
        "TOMATO_TEST_POSTGRES_URL",
        "postgres",
        "tomato_fixture",
    )
}

fn bulk_open_fixture(kind: &str, env: &str, scheme: &str, user: &str) -> anyhow::Result<()> {
    let url = std::env::var(env)?;
    let prefix = format!("{scheme}://{user}@127.0.0.1:");
    let endpoint = url.strip_prefix(&prefix).context("owned loopback only")?;
    let (port, database) = endpoint.split_once('/').context("owned database")?;
    let port = port.parse::<u16>()?;
    anyhow::ensure!(port > 0 && database == "tomato_fixture");
    let root = tempfile::tempdir()?;
    let config = root.path().join("connections.json");
    let repo = ConnectionFileRepository::new(&config);
    repo.save(&repo.load()?)?;
    let before = std::fs::read(&config)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repo);
    let mut controls = Controls {
        context: ctx,
        time: 10.,
    };
    let mut connection = Connection::new(kind);
    connection.set("host", "127.0.0.1");
    connection.set("port", port);
    connection.set("username", user);
    connection.set("database", database);
    connection.set("password", "");
    app.store.connections.push(connection.clone());
    let schema = format!(
        "tomato_objects_ui_{}",
        Connection::new("TOOL").id.replace('-', "")
    );
    let postgres = kind == "POSTGRESQL";
    let (namespace, suffix, display_database) = if postgres {
        ("SCHEMA", " CASCADE", database)
    } else {
        ("DATABASE", "", schema.as_str())
    };
    let quoted = tomato_services::dialect::resolve(kind)?.quote(&schema)?;
    let handle = app.runtime.as_ref().unwrap().handle().clone();
    let mut db = tomato_services::Databases::default();
    handle.block_on(db.query(&connection, &format!("CREATE {namespace} {quoted}")))?;
    let outcome = (|| -> anyhow::Result<()> {
        let ddl = (0..66)
            .map(|i| format!("CREATE TABLE {quoted}.item_{i:02}(id INTEGER PRIMARY KEY);"))
            .chain([
                format!("INSERT INTO {quoted}.item_00 VALUES (17);"),
                format!("CREATE VIEW {quoted}.items_view AS SELECT id FROM {quoted}.item_00;"),
            ])
            .collect::<String>();
        handle.block_on(db.query(&connection, &ddl))?;
        app.open_objects(connection.clone(), schema.clone());
        controls.wait(&mut app)?;
        let owner = app.active.clone().context("objects tab")?;
        anyhow::ensure!(
            workbench(&app.tabs[0])
                .browser
                .as_ref()
                .unwrap()
                .catalog
                .objects
                .len()
                == 67
        );
        controls.click(&mut app, "item_00", false, false)?;
        controls.frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
        );
        controls.click(&mut app, "打开表", false, false)?;
        anyhow::ensure!(app.tabs.len() == 68, "all selected objects must open");
        let first = app.tabs[1].id.clone();
        let last = app.tabs.last().unwrap();
        anyhow::ensure!(app.active.as_deref() == Some(last.id.as_str()));
        anyhow::ensure!(
            last.title == format!("items_view@{display_database}(127.0.0.1:{port})-视图")
        );
        anyhow::ensure!(
            app.tabs[1..67]
                .iter()
                .all(|tab| workbench(tab).object_load_pending && !tab.busy)
        );
        controls.wait(&mut app)?;
        let last = app.tabs.last().unwrap();
        anyhow::ensure!(
            workbench(last).table_is_view
                && workbench(last).schema == schema
                && workbench(last)
                    .page
                    .as_ref()
                    .is_some_and(|page| page.rows[0][0].display() == "17")
        );
        app.active = Some(first);
        controls.frame(&mut app, vec![]);
        controls.wait(&mut app)?;
        anyhow::ensure!(
            workbench(&app.tabs[1]).schema == schema
                && workbench(&app.tabs[1])
                    .page
                    .as_ref()
                    .is_some_and(|page| page.rows[0][0].display() == "17")
        );

        app.active = Some(owner.clone());
        controls.click(&mut app, "打开表", false, false)?;
        anyhow::ensure!(
            app.tabs.len() == 68 && app.pending.is_empty(),
            "opening again must reuse the tabs"
        );
        app.active = Some(owner);
        controls.click(&mut app, "item_00", true, false)?;
        controls.click(&mut app, "设计表", false, true)?;
        anyhow::ensure!(
            app.tabs.len() == 135,
            "right click must preserve all selected objects"
        );
        anyhow::ensure!(
            app.tabs[68..134]
                .iter()
                .all(|tab| workbench(tab).object_load_pending && !tab.busy)
        );
        controls.wait(&mut app)?;
        let last = app.tabs.last().unwrap();
        anyhow::ensure!(
            last.title == format!("items_view@{display_database}(127.0.0.1:{port})-视图结构")
        );
        anyhow::ensure!(
            workbench(last)
                .designer
                .as_ref()
                .is_some_and(|draft| draft.saved.is_some())
        );
        app.active = Some(app.tabs[68].id.clone());
        controls.frame(&mut app, vec![]);
        controls.wait(&mut app)?;
        anyhow::ensure!(
            workbench(&app.tabs[68])
                .designer
                .as_ref()
                .is_some_and(|draft| draft.saved.is_some())
        );
        anyhow::ensure!(std::fs::read(&config)? == before);
        Ok(())
    })();
    handle.block_on(db.query(&connection, &format!("DROP {namespace} {quoted}{suffix}")))?;
    outcome
}
