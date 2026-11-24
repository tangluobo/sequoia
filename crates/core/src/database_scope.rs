//! Select a database in an ephemeral session without replacing its physical endpoint.
use crate::Connection;
pub fn hive_name(value: &str) -> &str {
    let name = value
        .split([';', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if name.is_empty() { "default" } else { name }
}
pub fn select(connection: &Connection, database: &str) -> Connection {
    let mut scoped = connection.clone();
    if database.is_empty() {
        return scoped;
    }
    match connection.kind() {
        "SQLITE" => {}
        "ORACLE" => scoped.set("__tomatoOracleOwner", database),
        "HIVE" => {
            let configured = connection.text("database");
            let suffix = configured
                .find([';', '?', '#'])
                .map_or("", |at| &configured[at..]);
            scoped.set("database", format!("{}{suffix}", hive_name(database)));
        }
        _ => scoped.set("database", database),
    }
    scoped
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_namespace_preserves_credentials_unknown_fields_and_endpoint() {
        for kind in ["ORACLE", "HIVE", "MYSQL", "POSTGRESQL", "SQLITE"] {
            let mut connection = Connection::new(kind);
            let configured = if kind == "HIVE" {
                "default;auth=noSasl;transportMode=http?hive.execution.engine=tez#var=中文"
            } else {
                "/original-service"
            };
            connection.set("database", configured);
            connection.set("password", "fixture-only");
            connection.set("unknown-legacy", serde_json::json!({"items":[1,2,3]}));
            let before = serde_json::to_value(&connection).unwrap();
            let scoped = select(&connection, "selected");
            assert_eq!(scoped.id, connection.id);
            assert_eq!(scoped.text("password"), connection.text("password"));
            match kind {
                "ORACLE" => {
                    assert_eq!(scoped.text("database"), configured);
                    assert_eq!(scoped.text("__tomatoOracleOwner"), "selected");
                }
                "SQLITE" => assert_eq!(scoped.text("database"), configured),
                "HIVE" => assert_eq!(
                    scoped.text("database"),
                    "selected;auth=noSasl;transportMode=http?hive.execution.engine=tez#var=中文"
                ),
                _ => assert_eq!(scoped.text("database"), "selected"),
            }
            assert_eq!(serde_json::to_value(&connection).unwrap(), before);
            assert_eq!(
                serde_json::to_value(&scoped).unwrap()["unknown-legacy"],
                before["unknown-legacy"]
            );
        }
    }
}
