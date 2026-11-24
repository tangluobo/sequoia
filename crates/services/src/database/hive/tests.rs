use super::*;
use wire::Value;
mod java;
mod server;

#[tokio::test]
async fn thrift_fragmented_reply_checks_request_identity_and_bounds_before_allocating() -> Result<()>
{
    use tokio::io::AsyncWriteExt as _;
    let body = Value::object([
        (1, Value::object([(1, Value::I32(0))])),
        (2, Value::string("中文响应")),
    ]);
    let bytes = wire::reply("GetInfo", 31, body)?;
    assert_eq!(&bytes[..4], &[0x80, 1, 0, 2]);
    let (mut reader, mut writer) = tokio::io::duplex(1);
    let send = tokio::spawn(async move {
        for byte in bytes {
            writer.write_all(&[byte]).await.unwrap();
        }
    });
    let value = wire::response(&mut reader, "GetInfo", 31).await?;
    assert_eq!(value.required(2)?.text()?, "中文响应");
    send.await?;
    let bytes = wire::reply("GetInfo", 32, Value::object([]))?;
    assert!(
        wire::response(&mut bytes.as_slice(), "GetInfo", 31)
            .await
            .is_err()
    );
    assert!(
        wire::response(&mut bytes.as_slice(), "OpenSession", 32)
            .await
            .is_err()
    );
    let mut bytes = 0x8001_0002u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(&i32::MAX.to_be_bytes());
    assert!(
        wire::response(&mut bytes.as_slice(), "GetInfo", 0)
            .await
            .is_err()
    );
    Ok(())
}

#[test]
fn hive_parameter_tokens_preserve_quotes_comments_unicode_and_binary() -> Result<()> {
    let sql = "SELECT ?, '?', `问?号`, ? /* ? */ -- ?\n";
    let actual = bind::apply(
        sql,
        Some(vec![
            Cell::Text("O'Reilly\\中文".into()),
            Cell::Binary(vec![0, 255]),
        ]),
    )?;
    assert_eq!(
        actual,
        "SELECT 'O\\'Reilly\\\\中文', '?', `问?号`, unhex('00FF') /* ? */ -- ?\n"
    );
    assert!(bind::apply("SELECT ?", Some(vec![])).is_err());
    assert!(bind::apply("SELECT '?'", Some(vec![Cell::Null])).is_err());
    assert!(bind::apply("SELECT ?", Some(vec![Cell::Real(f64::NAN)])).is_err());
    Ok(())
}

#[test]
fn hive_columnar_results_keep_decimal_precision_binary_empty_and_null_distinct() -> Result<()> {
    let columns = vec![
        data::Column {
            name: "id".into(),
            kind: 4,
            jdbc: -5,
            data_type: "BIGINT".into(),
        },
        data::Column {
            name: "price".into(),
            kind: 15,
            jdbc: 3,
            data_type: "DECIMAL".into(),
        },
        data::Column {
            name: "payload".into(),
            kind: 9,
            jdbc: -2,
            data_type: "BINARY".into(),
        },
        data::Column {
            name: "missing".into(),
            kind: 7,
            jdbc: 12,
            data_type: "STRING".into(),
        },
    ];
    let vector = |kind, values, nulls| {
        Value::object([(kind, Value::object([(1, values), (2, Value::Bytes(nulls))]))])
    };
    let decimal = "12345678901234567890.123456789012345678";
    let rowset = Value::object([
        (1, Value::I64(0)),
        (2, Value::List(12, Vec::new())),
        (
            3,
            Value::List(
                12,
                vec![
                    vector(
                        5,
                        Value::List(10, vec![Value::I64(i64::MAX), Value::I64(i64::MIN)]),
                        vec![],
                    ),
                    vector(
                        7,
                        Value::List(11, vec![Value::string(decimal), Value::string("0.00")]),
                        vec![],
                    ),
                    vector(
                        8,
                        Value::List(11, vec![Value::Bytes(vec![0, 255]), Value::Bytes(vec![])]),
                        vec![],
                    ),
                    vector(
                        7,
                        Value::List(11, vec![Value::string(""), Value::string("")]),
                        vec![1],
                    ),
                ],
            ),
        ),
    ]);
    let rows = data::rows(rowset, &columns)?;
    assert_eq!(
        rows[0],
        vec![
            Cell::Integer(i64::MAX),
            Cell::Text(decimal.into()),
            Cell::Binary(vec![0, 255]),
            Cell::Null
        ]
    );
    assert_eq!(
        rows[1],
        vec![
            Cell::Integer(i64::MIN),
            Cell::Text("0.00".into()),
            Cell::Binary(vec![]),
            Cell::Text("".into())
        ]
    );
    Ok(())
}

#[test]
fn changing_hive_database_keeps_configured_session_parameters_and_saved_connection() -> Result<()> {
    let mut connection = Connection::new("HIVE");
    connection.set("host", "hive.internal.test");
    connection.set("database","original;auth=noSasl;transportMode=http;httpPath=custom;ssl=true;sslTrustStore=F%3A%2Fcerts%2Ftrusted%20ca.pem?hive.execution.engine=tez#label=中文");
    let before = serde_json::to_value(&connection)?;
    let config = config::Config::new(&connection, "selected", None)?;
    assert!(config.http() && config.ssl() && !config.sasl()?);
    assert_eq!(config.http_path()?, "/custom");
    assert_eq!(config.option("ssltruststore"), "F:/certs/trusted ca.pem");
    assert_eq!(config.configuration["use:database"], "selected");
    assert_eq!(
        config.configuration["set:hiveconf:hive.execution.engine"],
        "tez"
    );
    assert_eq!(config.configuration["set:hivevar:label"], "中文");
    assert_eq!(serde_json::to_value(&connection)?, before);
    Ok(())
}

#[test]
fn hive_password_prompt_obeys_the_native_authentication_configuration() {
    for (suffix, username, expected) in [
        ("default;auth=noSasl", "user", false),
        ("default;principal=hive/_HOST@TOMATO.TEST", "user", false),
        ("default", "user", true),
        ("default", "", false),
        ("default;auth=unsupported", "user", false),
    ] {
        let mut c = Connection::new("HIVE");
        c.set("host", "127.0.0.1");
        c.set("port", 10000);
        c.set("database", suffix);
        c.set("username", username);
        let original = serde_json::to_value(&c).unwrap();
        assert_eq!(
            crate::database::authentication_requires_password(&c),
            expected,
            "{suffix}"
        );
        assert_eq!(serde_json::to_value(&c).unwrap(), original);
    }
}
