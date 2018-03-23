extern crate chrono;
extern crate flexi_logger;
extern crate hdbconnect;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

mod test_utils;

use hdbconnect::{Connection, HdbResult};

#[test] // cargo test --test test_030_prepare -- --nocapture
pub fn test_030_prepare() {
    test_utils::init_logger("test_030_prepare=info");

    match impl_test_030_prepare() {
        Err(e) => {
            error!("test_030_prepare() failed with {:?}", e);
            assert!(false)
        }
        Ok(i) => info!("{} calls to DB were executed", i),
    }
}

// Test prepared statements, transactional correctness,
// incl. parameter serialization (and resultset deserialization)
fn impl_test_030_prepare() -> HdbResult<i32> {
    let mut connection = test_utils::get_authenticated_connection()?;

    prepare_insert_statement(&mut connection)?;

    Ok(connection.get_call_count()?)
}

fn prepare_insert_statement(connection: &mut Connection) -> HdbResult<()> {
    info!(
        "test statement preparation and transactional correctness (auto_commit on/off, rollbacks)"
    );
    connection.multiple_statements_ignore_err(vec!["drop table TEST_PREPARE"]);
    let stmts = vec!["create table TEST_PREPARE (F1_S NVARCHAR(20), F2_I INT)"];
    connection.multiple_statements(stmts)?;

    #[derive(Deserialize, Debug)]
    struct TestStruct {
        #[serde(rename = "F1_S")] f1_s: Option<String>,
        #[serde(rename = "F2_I")] f2_i: Option<i32>,
    }

    let insert_stmt_str = "insert into TEST_PREPARE (F1_S, F2_I) values(?, ?)";

    // prepare & execute
    let mut insert_stmt = connection.prepare(insert_stmt_str)?;
    insert_stmt.add_batch(&("conn1-auto1", 45_i32))?;
    insert_stmt.add_batch(&("conn1-auto2", 46_i32))?;
    insert_stmt.execute_batch()?;

    // prepare & execute on second connection
    let connection2 = connection.spawn()?;
    let mut insert_stmt2 = connection2.prepare(insert_stmt_str)?;
    insert_stmt2.add_batch(&("conn2-auto1", 45_i32))?;
    insert_stmt2.add_batch(&("conn2-auto2", 46_i32))?;
    insert_stmt2.execute_batch()?;

    // prepare & execute on first connection with auto_commit off,
    // rollback, do it again and commit
    connection.set_auto_commit(false)?;
    let mut insert_stmt = connection.prepare(insert_stmt_str)?;
    insert_stmt.add_batch(&("conn1-rollback1", 45_i32))?;
    insert_stmt.add_batch(&("conn1-rollback2", 46_i32))?;
    insert_stmt.add_batch(&("conn1-rollback3", 47_i32))?;
    insert_stmt.execute_batch()?;
    connection.rollback()?;

    insert_stmt.add_batch(&("conn1-commit1", 45_i32))?;
    insert_stmt.add_batch(&("conn1-commit2", 46_i32))?;
    insert_stmt.execute_batch()?;
    connection.commit()?;

    // prepare, execute batch, rollback in new spawn
    let mut connection3 = connection.spawn()?;
    let mut insert_stmt3 = connection3.prepare(insert_stmt_str)?;
    insert_stmt3.add_batch(&("conn3-auto1", 45_i32))?;
    insert_stmt3.add_batch(&("conn3-auto2", 46_i32))?;
    insert_stmt3.add_batch(&("conn3-auto3", 47_i32))?;
    insert_stmt3.execute_batch()?;
    connection3.rollback()?;

    let typed_result: Vec<TestStruct> = connection.query("select * from TEST_PREPARE")?.try_into()?;
    assert_eq!(typed_result.len(), 6);
    for ts in typed_result {
        let s = ts.f1_s.as_ref().unwrap();
        assert_eq!(false, s.contains("rollback"));
        assert_eq!(true, s.contains("comm") || s.contains("auto"));
    }
    Ok(())
}
