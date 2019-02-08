mod test_utils;

// use chrono::NaiveDateTime;
use flexi_logger::ReconfigurationHandle;
use hdbconnect::{Connection, HdbValue, HdbResult};
use log::{debug,info};
use serde_bytes::{ByteBuf,Bytes};
use serde_derive::Deserialize;

#[test] // cargo test --test test_041_datatypes_b -- --nocapture
pub fn test_041_datatypes_b() -> HdbResult<()> {
    let mut log_handle = test_utils::init_logger();
    let mut connection = test_utils::get_authenticated_connection()?;

    prepare(&mut log_handle, &mut connection)?;
    write(&mut log_handle, &mut connection)?;
    read(&mut log_handle, &mut connection)?;

    info!("{} calls to DB were executed", connection.get_call_count()?);

    Ok(())
}

fn prepare(_log_handle: &mut ReconfigurationHandle, connection: &mut Connection) -> HdbResult<()> {
    // prepare the db table
    connection.multiple_statements_ignore_err(vec!["drop table TEST_TYPES_B"]);
    connection.multiple_statements(vec![
        "create table TEST_TYPES_B ( \
         id BIGINT GENERATED BY DEFAULT AS IDENTITY primary key , \
         FIELD_CLOB CLOB, \
         FIELD_NCLOB NCLOB, \
         FIELD_BLOB BLOB, \
         FIELD_BOOLEAN BOOLEAN, \
         FIELD_TEXT TEXT, \
         FIELD_SHORTTEXT SHORTTEXT(99), \
         FIELD_LONGDATE LONGDATE, \
         FIELD_SECONDDATE SECONDDATE, \
         FIELD_DAYDATE DAYDATE, \
         FIELD_SECONDTIME SECONDTIME \
         )",
    ])?;
    Ok(())
}

fn write(_log_handle: &mut ReconfigurationHandle, connection: &mut Connection) -> HdbResult<()> {
    info!("insert values directly");
    connection.dml(
        "\
         insert into TEST_TYPES_B ( \
         FIELD_CLOB, FIELD_NCLOB, FIELD_BLOB, \
         FIELD_BOOLEAN, FIELD_SHORTTEXT, FIELD_TEXT, \
         FIELD_LONGDATE, FIELD_SECONDDATE, FIELD_DAYDATE, FIELD_SECONDTIME \
         ) values( \
         'Hello world!', 'Hello world!', '0123456789abcdef', \
         false, 'Hello world!', 'Hello world!', \
         '2019-01-18 01:02:03.456789', '2019-01-18 01:02:03', '2019-01-18', '01:02:03' \
         )",
    )?;

    info!("insert values via prep-statement");
    let mut stmt = connection.prepare(
        "\
         insert into TEST_TYPES_B ( \
         FIELD_CLOB, FIELD_NCLOB, FIELD_BLOB, \
         FIELD_BOOLEAN, FIELD_SHORTTEXT, FIELD_TEXT, \
         FIELD_LONGDATE, FIELD_SECONDDATE, FIELD_DAYDATE, FIELD_SECONDTIME \
         ) values(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.execute(&(
        "Hello world!",
        "Hello world!",
        Bytes::new(&parse_hex("0123456789abcdef")),
        false,
        "Hello world!",
        "Hello world!",
        "2019-01-18 01:02:03.456789",
        "2019-01-18 01:02:03",
        "2019-01-18",
        "01:02:03",
    ))?;

    stmt.execute_row(vec![
        HdbValue::STRING("foo bar rab oof".to_string()),
        HdbValue::STRING("foo bar rab oof".to_string()),
        HdbValue::STRING("foo bar rab oof".to_string()),
        HdbValue::STRING("true".to_string()),
        HdbValue::STRING("foo bar rab oof".to_string()),
        HdbValue::STRING("foo bar rab oof".to_string()),
        HdbValue::STRING("2019-01-31 04:04:04.400000000".to_string()),
        HdbValue::STRING("2019-01-31 04:04:04".to_string()),
        HdbValue::STRING("2019-01-31".to_string()),
        HdbValue::STRING("04:04:04".to_string()),
    ])?;

    info!("insert nulls directly");
    connection.dml(
        "\
         insert into TEST_TYPES_B ( \
         FIELD_CLOB, FIELD_NCLOB, FIELD_BLOB, \
         FIELD_BOOLEAN, FIELD_SHORTTEXT, FIELD_TEXT, \
         FIELD_LONGDATE, FIELD_SECONDDATE, FIELD_DAYDATE, FIELD_SECONDTIME \
         ) values( \
         NULL, NULL, NULL, \
         NULL, NULL, NULL, \
         NULL, NULL, NULL, NULL \
         )",
    )?;

    info!("insert nulls via prep-statement");
    stmt.execute_row(vec![
        HdbValue::NULL, HdbValue::NULL, HdbValue::NULL, HdbValue::NULL, HdbValue::NULL, 
        HdbValue::NULL, HdbValue::NULL, HdbValue::NULL, HdbValue::NULL, HdbValue::NULL,
    ])?;
    Ok(())
}


    #[derive(Debug, Deserialize)]
    #[allow(non_snake_case)]
    struct Data {
        ID: u32,
        FIELD_CLOB: Option<String>,
        FIELD_NCLOB: Option<String>,
        FIELD_BLOB: Option<ByteBuf>,
        FIELD_BOOLEAN: Option<bool>,
        FIELD_TEXT: Option<String>,
        FIELD_SHORTTEXT: Option<String>,
        FIELD_LONGDATE: Option<chrono::NaiveDateTime>,
        FIELD_SECONDDATE: Option<chrono::NaiveDateTime>,
        FIELD_DAYDATE: Option<chrono::NaiveDate>,
        FIELD_SECONDTIME: Option<chrono::NaiveTime>,
    }


fn read(_log_handle: &mut ReconfigurationHandle, connection: &mut Connection) -> HdbResult<()> {
    {
        info!("read non-null values and evaluate via serde_db");
        let q = "select * from TEST_TYPES_B where id = 1";
        let data: Data = connection.query(q)?.try_into()?;
        debug!("data: {:?}", data);
    }
    {
        info!("read null values and evaluate via serde_db");
        let q = "select * from TEST_TYPES_B where id = 3";
        let data: Data = connection.query(q)?.try_into()?;
        debug!("data: {:?}", data);
    }
    {
        info!("read non-null values and evaluate directly");
        let q = "select * from TEST_TYPES_B where id = 1";
        let mut data = connection.query(q)?;
        debug!("data: {:?}", data);
        let row = data.next_row()?.unwrap();
        for value in row {
            assert!(!value.is_null());
        }
    }
    {
        info!("read null values and evaluate directly");
        let q = "select * from TEST_TYPES_B where id = 4";
        let mut resultset = connection.query(q)?;
        debug!("resultset: {:?}", resultset);
        let row = resultset.next_row()?.unwrap();
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
            b'0'...b'9' => Some(b - b'0'),
            b'a'...b'f' => Some(b - b'a' + 10),
            b'A'...b'F' => Some(b - b'A' + 10),
            _ => None,
        })
        .fuse();

    let mut bytes = Vec::new();
    while let (Some(h), Some(l)) = (hex_bytes.next(), hex_bytes.next()) {
        bytes.push(h << 4 | l)
    }
    bytes
}
