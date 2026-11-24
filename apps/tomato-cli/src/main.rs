use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::{io::Read, path::PathBuf};
use tomato_core::{
    Store,
    repository::{ConnectionFileRepository, WorkspaceRepository},
};
use tomato_services::commands::{Command, CommandHandler, Engine};
mod input_share;

#[derive(Parser)]
#[command(version, about = "Tomato 原生工作台命令行，与 GUI 共用业务引擎")]
struct Args {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// 键鼠共享会话；从标准输入逐行读取 Action JSON，Stop 或 EOF 退出
    InputShare,
    /// 只读检查配置兼容性，仅报告数量，不输出主机或凭据
    CheckConfig,
    /// 列出连接（敏感字段自动移除）
    Connections,
    /// 导入 Java 版加密配置或 JSON 连接导出
    Import {
        path: PathBuf,
    },
    Export {
        path: PathBuf,
    },
    /// 对指定连接执行 SQL；密码可由 TOMATO_PASSWORD 环境变量提供
    Sql {
        id: String,
        #[arg(long, conflicts_with = "file")]
        query: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// 从标准输入读取一个 JSON Command，对应 services::commands::Command
    Run,
    /// JSON 格式化、压缩、转义和 Unicode 转换
    Json {
        #[arg(default_value = "format")]
        action: String,
        path: Option<PathBuf>,
    },
    Ls {
        path: PathBuf,
    },
    ConvertImage {
        source: PathBuf,
        destination: PathBuf,
        #[arg(long, default_value_t = 0)]
        width: u32,
    },
    ConvertDataset {
        source: PathBuf,
        destination: PathBuf,
    },
    Download {
        url: String,
        destination: PathBuf,
    },
    /// 列出连接类型、原生适配器状态和数据库产品
    Catalog,
    /// 无界面验证 JSON、SQLite、事务和多语句查询
    SelfCheck,
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(code) = tomato_services::data_import::worker_entry() {
        std::process::exit(code);
    }
    tomato_services::init_crypto();
    let args = Args::parse();
    let repository = ConnectionFileRepository::new(args.config.unwrap_or_else(Store::default_path));
    let mut engine = Engine::default();
    let command = match args.command {
        Action::InputShare => return input_share::run(&mut engine).await,
        Action::CheckConfig => {
            let store = repository.load()?;
            let folders = store
                .connections
                .iter()
                .filter(|item| item.kind.is_none())
                .count();
            println!(
                "配置兼容性检查通过：{} 个连接，{} 个分组（只读，未连接服务器）",
                store.connections.len() - folders,
                folders
            );
            return Ok(());
        }
        Action::Connections => {
            println!("{}", repository.load()?.export()?);
            return Ok(());
        }
        Action::Import { path } => {
            use tomato_services::connections::{Output, Request};
            let store = repository.load()?;
            let Output::Preview { plan, .. } = tomato_services::connections::execute(
                Request::Preview {
                    source: path.clone(),
                    baseline: tomato_core::config::ImportBaseline::new(&store)?,
                },
                Default::default(),
            )?
            else {
                unreachable!()
            };
            let selected: std::collections::BTreeSet<_> = (0..plan.connections.len())
                .filter(|index| !plan.duplicate(*index))
                .collect();
            let count = if selected.is_empty() {
                0
            } else {
                let Output::Committed { count, .. } = tomato_services::connections::execute(
                    Request::Commit {
                        source: path,
                        source_signature: plan.source_signature(),
                        guard: repository.guard()?,
                        selected,
                        prepared: Some(plan),
                    },
                    Default::default(),
                )?
                else {
                    unreachable!()
                };
                count
            };
            println!("已导入 {count} 个连接，已按原版兼容格式加密保存");
            return Ok(());
        }
        Action::Export { path } => {
            repository.export(&repository.load()?, &path)?;
            return Ok(());
        }
        Action::Sql { id, query, file } => {
            let store = repository.load()?;
            let mut connection = store
                .connections
                .iter()
                .find(|item| item.id == id)
                .context("未找到连接 ID")?
                .clone();
            if let Ok(password) = std::env::var("TOMATO_PASSWORD") {
                connection.set("password", password);
            }
            let sql = match (query, file) {
                (Some(query), _) => query,
                (_, Some(path)) => std::fs::read_to_string(path)?,
                _ => read_stdin()?,
            };
            Command::Sql { connection, sql }
        }
        Action::Run => {
            serde_json::from_str(&read_stdin()?).context("标准输入不是有效 Command JSON")?
        }
        Action::Json { action, path } => {
            let text = match path {
                Some(path) => std::fs::read_to_string(path)?,
                None => read_stdin()?,
            };
            println!("{}", tomato_core::tools::json_transform(&text, &action)?);
            return Ok(());
        }
        Action::Ls { path } => Command::ListFiles { path },
        Action::ConvertImage {
            source,
            destination,
            width,
        } => Command::ConvertImage {
            source,
            destination,
            width,
        },
        Action::ConvertDataset {
            source,
            destination,
        } => Command::ConvertDataset {
            source,
            destination,
        },
        Action::Download { url, destination } => Command::Download { url, destination },
        Action::Catalog => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"connections":tomato_core::catalog::KINDS,"database_profiles":tomato_core::catalog::DATABASE_PROFILES,"tools":tomato_core::catalog::TOOLS})
                )?
            );
            return Ok(());
        }
        Action::SelfCheck => {
            self_check(&mut engine).await?;
            println!("Tomato self-check passed");
            return Ok(());
        }
    };
    let output = engine.execute(command).await?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn read_stdin() -> Result<String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

async fn self_check(engine: &mut Engine) -> Result<()> {
    let connection = tomato_core::Connection::new("SQLITE");
    engine.execute(Command::Sql { connection: connection.clone(), sql: "CREATE TABLE check_table(id INTEGER, name TEXT); INSERT INTO check_table VALUES(1,'中文'); BEGIN; UPDATE check_table SET name='bad'; ROLLBACK;".into() }).await?;
    let result = engine
        .execute(Command::Sql {
            connection,
            sql: "SELECT name FROM check_table;".into(),
        })
        .await?;
    match result {
        tomato_services::commands::Output::Queries(results)
            if results
                .first()
                .and_then(|r| r.rows.first())
                .and_then(|r| r.first())
                .is_some_and(|cell| cell == "中文") => {}
        _ => bail!("SQLite self-check returned unexpected data"),
    }
    if tomato_core::tools::json_transform("{\"test\": true}", "minify")? != "{\"test\":true}" {
        bail!("JSON self-check failed");
    }
    Ok(())
}
