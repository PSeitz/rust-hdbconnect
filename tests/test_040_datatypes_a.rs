#[macro_use]
extern crate serde_derive;

mod test_utils;

use bigdecimal::BigDecimal;
use flexi_logger::ReconfigurationHandle;
use hdbconnect::{Connection, HdbResult, HdbValue};
use log::{debug, info};
use serde_bytes::{ByteBuf, Bytes};
use serde_derive::Deserialize;

#[test] // cargo test --test <test_040_datatypes_a> -- --nocapture
pub fn test_040_datatypes_a() -> HdbResult<()> {
    let mut log_handle = test_utils::init_logger();
    let start = std::time::Instant::now();
    let mut connection = test_utils::get_authenticated_connection()?;

    prepare(&mut log_handle, &mut connection)?;
    write(&mut log_handle, &mut connection)?;
    read(&mut log_handle, &mut connection)?;

    test_utils::closing_info(connection, start)
}

fn prepare(_log_handle: &mut ReconfigurationHandle, connection: &mut Connection) -> HdbResult<()> {
    info!("prepare the db table");
    connection.multiple_statements_ignore_err(vec!["drop table TEST_TYPES_A"]);
    connection.multiple_statements(vec![
        "create table TEST_TYPES_A ( \
         id BIGINT GENERATED BY DEFAULT AS IDENTITY primary key , \
         FIELD_TINYINT TINYINT, \
         FIELD_SMALLINT SMALLINT, \
         FIELD_INT INT, \
         FIELD_BIGINT BIGINT, \
         FIELD_SMALLDECIMAL SMALLDECIMAL, \
         FIELD_DECIMAL DECIMAL, \
         FIELD_REAL REAL, \
         FIELD_DOUBLE DOUBLE, \
         FIELD_CHAR CHAR(12), \
         FIELD_VARCHAR VARCHAR(12), \
         FIELD_NCHAR NCHAR(12), \
         FIELD_NVARCHAR NVARCHAR(12), \
         FIELD_BINARY BINARY(8), \
         FIELD_VARBINARY VARBINARY(8) \
         )",
    ])?;
    Ok(())
}

fn write(_log_handle: &mut ReconfigurationHandle, connection: &mut Connection) -> HdbResult<()> {
    info!("insert values directly");
    connection.dml(
        "\
         insert into TEST_TYPES_A \
         ( \
         FIELD_TINYINT, FIELD_SMALLINT, FIELD_INT, FIELD_BIGINT, \
         FIELD_SMALLDECIMAL, FIELD_DECIMAL, FIELD_REAL, FIELD_DOUBLE, \
         FIELD_CHAR, FIELD_VARCHAR, FIELD_NCHAR, FIELD_NVARCHAR, \
         FIELD_BINARY, FIELD_VARBINARY \
         ) values( \
         1, 1, 1, 1, \
         1.0, 1.0, 1.0, 1.0, \
         'Hello world!', 'Hello world!', 'Hello world!', 'Hello world!', \
         '0123456789abcdef', '0123456789abcdef' \
         )",
    )?;

    info!("insert values via prep-statement");
    let mut stmt = connection.prepare(
        "\
         insert into TEST_TYPES_A \
         ( \
         FIELD_TINYINT, FIELD_SMALLINT, FIELD_INT, FIELD_BIGINT, \
         FIELD_SMALLDECIMAL, FIELD_DECIMAL, FIELD_REAL, FIELD_DOUBLE, \
         FIELD_CHAR, FIELD_VARCHAR, FIELD_NCHAR, FIELD_NVARCHAR, \
         FIELD_BINARY, FIELD_VARBINARY \
         ) values(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.execute(&(
        1,
        1,
        1,
        1,
        1.0,
        1.0,
        1.0_f32,
        1.0,
        "Hello world!",
        "Hello world!",
        "Hello world!",
        "Hello world!",
        Bytes::new(&parse_hex("0123456789abcdef")),
        Bytes::new(&parse_hex("0123456789abcdef")),
    ))?;

    stmt.execute_row(vec![
        HdbValue::STRING("2".to_string()),
        HdbValue::STRING("2".to_string()),
        HdbValue::STRING("2".to_string()),
        HdbValue::STRING("2".to_string()),
        HdbValue::STRING("2.0".to_string()),
        HdbValue::STRING("2.0".to_string()),
        HdbValue::STRING("2.0".to_string()),
        HdbValue::STRING("2.0".to_string()),
        HdbValue::STRING("Hello world!".to_string()),
        HdbValue::STRING("Hello world!".to_string()),
        HdbValue::STRING("Hello world!".to_string()),
        HdbValue::STRING("Hello world!".to_string()),
        HdbValue::STRING("deadbeef01234567".to_string()),
        HdbValue::STRING("deadbeef01234567".to_string()),
    ])?;

    info!("insert nulls directly");
    connection.dml(
        "insert into TEST_TYPES_A \
         ( \
         FIELD_TINYINT, FIELD_SMALLINT, FIELD_INT, FIELD_BIGINT, \
         FIELD_SMALLDECIMAL, FIELD_DECIMAL, FIELD_REAL, FIELD_DOUBLE, \
         FIELD_CHAR, FIELD_VARCHAR, FIELD_NCHAR, FIELD_NVARCHAR, \
         FIELD_BINARY, FIELD_VARBINARY \
         ) values( \
         NULL, NULL, NULL, NULL, \
         NULL, NULL, NULL, NULL, \
         NULL, NULL, NULL, NULL, \
         NULL, NULL \
         )",
    )?;

    info!("insert nulls via prep-statement");
    stmt.execute(&std::iter::repeat(()).take(14).collect::<Vec<_>>())?;

    info!("insert nulls via prep-statement, using HdbValue::NULL");
    stmt.execute_row(vec![
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
        HdbValue::NULL,
    ])?;
    Ok(())
}

fn read(_log_handle: &mut ReconfigurationHandle, connection: &mut Connection) -> HdbResult<()> {
    #[derive(Debug, Deserialize)]
    #[allow(non_snake_case)]
    struct Data {
        ID: u32,
        FIELD_TINYINT: Option<i8>,
        FIELD_SMALLINT: Option<i16>,
        FIELD_INT: Option<i32>,
        FIELD_BIGINT: Option<i64>,
        FIELD_SMALLDECIMAL: Option<f64>,
        FIELD_DECIMAL: Option<BigDecimal>,
        FIELD_REAL: Option<f32>,
        FIELD_DOUBLE: Option<f64>,
        FIELD_CHAR: Option<String>,
        FIELD_VARCHAR: Option<String>,
        FIELD_NCHAR: Option<String>,
        FIELD_NVARCHAR: Option<String>,
        FIELD_BINARY: Option<ByteBuf>,
        FIELD_VARBINARY: Option<ByteBuf>,
    }

    {
        info!("read non-null values and evaluate via serde_db");
        let q = "select * from TEST_TYPES_A where id = 1";
        let data: Data = connection.query(q)?.try_into()?;
        debug!("data: {:?}", data);
    }
    {
        info!("read null values and evaluate via serde_db");
        let q = "select * from TEST_TYPES_A where id = 3";
        let data: Data = connection.query(q)?.try_into()?;
        debug!("data: {:?}", data);
    }
    {
        info!("read non-null values and evaluate directly");
        let q = "select * from TEST_TYPES_A where id = 1";
        let mut resultset = connection.query(q)?;
        debug!("resultset: {:?}", resultset);
        let row = resultset.next_row()?.unwrap();
        for value in row {
            assert!(!value.is_null());
        }
    }
    {
        info!("read null values and evaluate directly");
        let q = "select * from TEST_TYPES_A where id = 4";
        let row = connection.query(q)?.into_single_row()?;
        assert!(!row[0].is_null());
        for value in row.skip(1) {
            assert!(value.is_null());
        }
    }
    Ok(())
}

fn parse_hex(hex_asm: &str) -> Vec<u8> {
    let mut hex_bytes = hex_asm
        .as_bytes()
        .iter()
        .filter_map(|b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        })
        .fuse();

    let mut bytes = Vec::new();
    while let (Some(h), Some(l)) = (hex_bytes.next(), hex_bytes.next()) {
        bytes.push(h << 4 | l)
    }
    bytes
}
