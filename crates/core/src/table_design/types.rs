//! Immutable original DataTypeProvider catalogs; no per-widget allocation.
use super::Dialect;
const MYSQL_BASE: &[&str] = &[
    "BIGINT",
    "BINARY",
    "BIT",
    "BLOB",
    "BOOL",
    "BOOLEAN",
    "CHAR",
    "DATE",
    "DATETIME",
    "DECIMAL",
    "DOUBLE",
    "ENUM",
    "FLOAT",
    "INT",
    "LONGBLOB",
    "LONGTEXT",
    "MEDIUMBLOB",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "NUMERIC",
    "SET",
    "SMALLINT",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TINYBLOB",
    "TINYINT",
    "TINYTEXT",
    "VARBINARY",
    "VARCHAR",
    "YEAR",
];
const MYSQL_JSON: &[&str] = &[
    "BIGINT",
    "BINARY",
    "BIT",
    "BLOB",
    "BOOL",
    "BOOLEAN",
    "CHAR",
    "DATE",
    "DATETIME",
    "DECIMAL",
    "DOUBLE",
    "ENUM",
    "FLOAT",
    "INT",
    "JSON",
    "LONGBLOB",
    "LONGTEXT",
    "MEDIUMBLOB",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "NUMERIC",
    "SET",
    "SMALLINT",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TINYBLOB",
    "TINYINT",
    "TINYTEXT",
    "VARBINARY",
    "VARCHAR",
    "YEAR",
];
const POSTGRES_BASE: &[&str] = &[
    "bigint",
    "bigserial",
    "bit",
    "bit varying",
    "boolean",
    "box",
    "bytea",
    "character",
    "character varying",
    "cidr",
    "circle",
    "date",
    "daterange",
    "decimal",
    "double precision",
    "inet",
    "int4range",
    "int8range",
    "integer",
    "interval",
    "json",
    "jsonb",
    "line",
    "lseg",
    "macaddr",
    "macaddr8",
    "money",
    "numeric",
    "numrange",
    "path",
    "point",
    "polygon",
    "real",
    "serial",
    "smallint",
    "smallserial",
    "text",
    "time",
    "timestamp",
    "timestamptz",
    "timetz",
    "tsrange",
    "tstzrange",
    "uuid",
    "xml",
];
const POSTGRES_MULTIRANGE: &[&str] = &[
    "bigint",
    "bigserial",
    "bit",
    "bit varying",
    "boolean",
    "box",
    "bytea",
    "character",
    "character varying",
    "cidr",
    "circle",
    "date",
    "datemultirange",
    "daterange",
    "decimal",
    "double precision",
    "inet",
    "int4multirange",
    "int4range",
    "int8multirange",
    "int8range",
    "integer",
    "interval",
    "json",
    "jsonb",
    "line",
    "lseg",
    "macaddr",
    "macaddr8",
    "money",
    "numeric",
    "nummultirange",
    "numrange",
    "path",
    "point",
    "polygon",
    "real",
    "serial",
    "smallint",
    "smallserial",
    "text",
    "time",
    "timestamp",
    "timestamptz",
    "timetz",
    "tsmultirange",
    "tsrange",
    "tstzmultirange",
    "tstzrange",
    "uuid",
    "xml",
];
const ORACLE_BASE: &[&str] = &[
    "BFILE",
    "BINARY_DOUBLE",
    "BINARY_FLOAT",
    "BLOB",
    "CHAR",
    "CLOB",
    "DATE",
    "FLOAT",
    "INTERVAL DAY TO SECOND",
    "INTERVAL YEAR TO MONTH",
    "LONG",
    "LONG RAW",
    "NCHAR",
    "NCLOB",
    "NUMBER",
    "NVARCHAR2",
    "RAW",
    "ROWID",
    "TIMESTAMP",
    "TIMESTAMP WITH LOCAL TIME ZONE",
    "TIMESTAMP WITH TIME ZONE",
    "UROWID",
    "VARCHAR2",
    "XMLType",
];
const ORACLE_JSON: &[&str] = &[
    "BFILE",
    "BINARY_DOUBLE",
    "BINARY_FLOAT",
    "BLOB",
    "CHAR",
    "CLOB",
    "DATE",
    "FLOAT",
    "INTERVAL DAY TO SECOND",
    "INTERVAL YEAR TO MONTH",
    "JSON",
    "LONG",
    "LONG RAW",
    "NCHAR",
    "NCLOB",
    "NUMBER",
    "NVARCHAR2",
    "RAW",
    "ROWID",
    "TIMESTAMP",
    "TIMESTAMP WITH LOCAL TIME ZONE",
    "TIMESTAMP WITH TIME ZONE",
    "UROWID",
    "VARCHAR2",
    "XMLType",
];
const HIVE: &[&str] = &[
    "ARRAY",
    "BIGINT",
    "BINARY",
    "BOOLEAN",
    "CHAR",
    "DATE",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "INT",
    "INTERVAL_DAY_TIME",
    "INTERVAL_YEAR_MONTH",
    "MAP",
    "SMALLINT",
    "STRING",
    "STRUCT",
    "TIMESTAMP",
    "TIMESTAMP WITH LOCAL TIME ZONE",
    "TINYINT",
    "UNIONTYPE",
    "VARCHAR",
];

fn version_parts(version: &str) -> (i32, i32) {
    let mut values = [Some(0_i32); 2];
    let mut part = 0;
    for byte in version
        .bytes()
        .filter(|ch| ch.is_ascii_digit() || *ch == b'.')
    {
        if byte == b'.' {
            part += 1;
            if part == 2 {
                break;
            }
        } else {
            values[part] = values[part]
                .and_then(|value| value.checked_mul(10))
                .and_then(|value| value.checked_add(i32::from(byte - b'0')));
        }
    }
    (values[0].unwrap_or(0), values[1].unwrap_or(0))
}
pub fn data_types(dialect: Dialect, version: Option<&str>) -> &'static [&'static str] {
    let (major, minor) = version.map(version_parts).unwrap_or_default();
    match dialect {
        Dialect::Mysql if version.is_none() || major > 5 || major == 5 && minor >= 7 => MYSQL_JSON,
        Dialect::Mysql => MYSQL_BASE,
        Dialect::Postgres if version.is_none() || major >= 14 => POSTGRES_MULTIRANGE,
        Dialect::Postgres => POSTGRES_BASE,
        Dialect::Oracle if version.is_none() || major >= 21 => ORACLE_JSON,
        Dialect::Oracle => ORACLE_BASE,
        Dialect::Hive => HIVE,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogs_match_original_version_gates_sorting_and_invalid_version_fallbacks() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/table-design/original-types.json"
        ))
        .unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 52);
        for row in rows.as_array().unwrap() {
            let dialect: Dialect = serde_json::from_value(row["kind"].clone()).unwrap();
            let types = data_types(dialect, row["version"].as_str());
            if row["error"].is_string() {
                // Original split(".") indexes an empty array for a dots-only
                // version. Native treats malformed versions as unknown/old.
                assert!(!types.is_empty());
            } else {
                let expected: Vec<_> = row["types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert_eq!(types, expected, "{} {}", row["kind"], row["version"]);
            }
        }
    }
}
