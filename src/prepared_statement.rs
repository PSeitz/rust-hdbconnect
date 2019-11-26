use crate::conn_core::AmConnCore;
use crate::protocol::argument::Argument;
use crate::protocol::part::Part;
use crate::protocol::partkind::PartKind;
use crate::protocol::parts::hdb_value::HdbValue;
use crate::protocol::parts::lob_flags::LobFlags;
use crate::protocol::parts::parameter_descriptor::ParameterDescriptors;
use crate::protocol::parts::parameter_rows::ParameterRows;
use crate::protocol::parts::resultset_metadata::ResultSetMetadata;
use crate::protocol::parts::type_id::TypeId;
use crate::protocol::request::{Request, HOLD_CURSORS_OVER_COMMIT};
use crate::protocol::request_type::RequestType;
use crate::protocol::server_usage::ServerUsage;
use crate::types_impl::lob::LobWriter;
use crate::{HdbError, HdbResponse, HdbResult};
use serde;
use serde_db::ser::SerializationError;
use std::io::Write;
use std::mem;
use std::sync::{Arc, Mutex};

/// Allows injection-safe SQL execution and repeated calls of the same statement
/// with different parameters with as few roundtrips as possible.
///
/// # Providing Input Parameters
///
/// ## Type systems
///
/// There are four typesystems involved!
/// * The _database type system_ consists of the standard SQL types and proprietary types
///   to represent values,
///   like TINYINT, FLOAT, NVARCHAR, and many others.
///   This type system is NOT directly visible to the client.
/// * The _wire_ has its own type system - it's focus is on efficient data transfer.
///   hdbconnect has to deal with these types internally.
///   `hdbconnect::TypeId` enumerates a somewhat reduced superset of the server-side
///   and the wire type system.
/// * The _driver API_ represents values with the `enum` `HdbValue`; this type system
///   hides the complexity of the wire type system and aims to be as close to the rust type system
///   as possible.
/// * The application is written in rust, and uses the _rust type system_.
///
/// ## From Rust to HdbValue
///
/// Prepared statements typically take one or more input parameter(s).
/// As part of the statement preparation, the database server provides the client
/// with detailed metadata for these parameters, which are kept by the `PreparedStatement`.
///
/// The parameter values can be handed over to the `PreparedStatement` either as
/// `Serializable` rust types, or explicitly as `HdbValue` instances.
/// If they are handed over as `Serializable` rust types, then the built-in `serde_db`-based
/// conversion will convert them directly into those `HdbValue` variants
/// that correspond to the `TypeId` that the server has requested.
/// The application can also provide the values explicitly as `HdbValue` instances and by that
/// enforce the usage of a different wire type and of server-side type conversions.
///
/// ## Sending HdbValues to the database
///
/// Sending an HdbValue::DECIMAL e.g. to the database can occur in different formats:
/// with older HANA versions, a proprietary DECIMAL format is used that is independent
/// of the number range of the concrete field. In newer HANA versions, three different
/// formats are used (FIXED8, FIXED12 and FIXED16) that together allow for a wider value
/// range and a lower bandwidth.
///
/// Similarly, a HdbValue::STRING is used to transfer values to all string-like wire types.
/// But the wire protocol sometimes also allow sending data in another wire type than requested.
/// If the database e.g. requests an INT, you can also send a String representation of the
/// number, by using `HdbValue::STRING("1088")`, instead of the binary INT representation
/// `HdbValue::INT(1088)`.
#[derive(Debug)]
pub struct PreparedStatement {
    am_ps_core: Arc<Mutex<PreparedStatementCore>>,
    server_usage: ServerUsage,
    a_descriptors: Arc<ParameterDescriptors>,
    o_a_rsmd: Option<Arc<ResultSetMetadata>>,
    batch: ParameterRows<'static>,
    _o_table_location: Option<Vec<i32>>,
}

pub type AmPsCore = Arc<Mutex<PreparedStatementCore>>;

#[derive(Debug)]
pub struct PreparedStatementCore {
    am_conn_core: AmConnCore,
    statement_id: u64,
}

impl<'a> PreparedStatement {
    /// Converts the input into a row of parameters,
    /// if it is consistent with the metadata, and
    /// executes the statement with these parameters immediately.
    ///
    /// The input conversion is done with the help of serde, so the input must implement
    /// `serde::ser::Serialize`.
    ///
    /// ```rust,no_run
    /// # use hdbconnect::{Connection, HdbResult, IntoConnectParams};
    /// # fn main() -> HdbResult<()> {
    /// # let params = "hdbsql://my_user:my_passwd@the_host:2222"
    /// #     .into_connect_params()
    /// #     .unwrap();
    /// # let mut connection = Connection::new(params).unwrap();
    /// let mut statement = connection.prepare("select * from phrases where ID = ? and text = ?")?;
    /// let hdbresponse = statement.execute(&(42, "Foo is bar"))?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// If the statement has no parameter, you can execute it like this
    ///
    /// ```rust, no_run
    /// # use hdbconnect::{Connection, HdbResult, IntoConnectParams, Row};
    /// # fn main() { }
    /// # fn foo() -> HdbResult<()> {
    /// # let mut connection = Connection::new("".into_connect_params()?)?;
    /// # let mut stmt = connection.prepare("")?;
    /// let hdbresponse = stmt.execute(&())?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// or like this
    ///
    /// ```rust, no_run
    /// # use hdbconnect::{Connection, HdbResult, IntoConnectParams, Row};
    /// # fn main() { }
    /// # fn foo() -> HdbResult<()> {
    /// # let mut connection = Connection::new("".into_connect_params()?)?;
    /// # let mut stmt = connection.prepare("")?;
    /// let hdbresponse = stmt.execute_batch()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn execute<T: serde::ser::Serialize>(&mut self, input: &T) -> HdbResult<HdbResponse> {
        trace!("PreparedStatement::execute()");
        if self.a_descriptors.has_in() {
            let mut par_rows = ParameterRows::new();
            par_rows.push(input, &self.a_descriptors)?;
            return self.execute_parameter_rows(Some(par_rows));
        }
        self.execute_parameter_rows(None)
    }

    /// Consumes the given HdbValues as a row of parameters for immediate execution.
    ///
    /// While in most cases
    /// [`PreparedStatement::execute()`](struct.PreparedStatement.html#method.execute)
    /// might be more convenient, streaming LOBs to the database is an important exception -
    /// it only works with this method!
    ///
    /// ## Example for streaming LOBs to the database
    ///
    /// Note that streaming LOBs to the database only works if auto-commit is switched off.
    /// Consequently, you need to commit the update explicitly.
    ///
    /// The first parameter in this example inserts a string into a normal NVARCHAR column;
    /// if the content is big (NVARCHAR takes up to 5000 characters),
    /// this may be a bit more efficient than `HdbValue::STRING`
    /// if you can avoid cloning the String.
    ///
    /// The second parameter uses a mutable reference to a reader which is supposed to produce
    /// the content you want to store in the LOB.
    ///
    /// ``` rust, no_run
    /// # use hdbconnect::{Connection, HdbValue, HdbResult, IntoConnectParams};
    /// # use std::sync::{Arc,Mutex};
    /// # fn main() -> HdbResult<()> {
    /// # let mut connection = Connection::new("".into_connect_params()?)?;
    ///   connection.set_auto_commit(false)?;
    /// # let insert_stmt_string = "insert into TEST_NCLOBS values(?, ?)".to_owned();
    ///   let mut stmt = connection.prepare(&insert_stmt_string)?;
    /// # let b = Vec::<u8>::new();
    /// # let mut reader = &b[..];
    /// # let reader = std::io::Cursor::new("foo bar");
    ///   stmt.execute_row(vec![
    ///       HdbValue::STR("nice descriptive text, could be quite long"),
    ///       HdbValue::LOBSTREAM(Some(Arc::new(Mutex::new(reader)))), // reader must implement std::io::Read
    ///   ])?;
    ///   connection.commit()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// `PreparedStatement::execute_row()` first executes the specified statement, with the given
    ///  parameter values, where LOBSTREAM instances are replaced with placeholders.
    /// After this call, the data from the readers are transferred to the database in additional
    /// roundtrips.
    pub fn execute_row(&'a mut self, hdb_values: Vec<HdbValue<'a>>) -> HdbResult<HdbResponse> {
        if self.a_descriptors.has_in() {
            let mut request = Request::new(RequestType::Execute, HOLD_CURSORS_OVER_COMMIT);

            request.push(Part::new(
                PartKind::StatementId,
                Argument::StatementId(self.am_ps_core.lock()?.statement_id),
            ));

            // If readers were provided, pick them out and replace them with None
            let mut readers: Vec<(HdbValue, TypeId)> = vec![];
            let hdb_values = hdb_values
                .into_iter()
                .zip(self.a_descriptors.iter_in())
                .map(|(v, d)| {
                    if let HdbValue::LOBSTREAM(Some(_)) = v {
                        readers.push((v, d.type_id()));
                        HdbValue::LOBSTREAM(None)
                    } else {
                        v
                    }
                })
                .collect();

            let mut par_rows = ParameterRows::new();
            par_rows.push_hdb_values(hdb_values, &self.a_descriptors)?;
            request.push(Part::new(
                PartKind::Parameters,
                Argument::Parameters(par_rows),
            ));

            if self
                .am_ps_core
                .lock()?
                .am_conn_core
                .lock()?
                .connect_options()
                .get_implicit_lob_streaming()
                .unwrap_or(false)
            {
                request.push(Part::new(
                    PartKind::LobFlags,
                    Argument::LobFlags(LobFlags::for_implicit_streaming()),
                ));
            }

            let mut main_reply = self.am_ps_core.lock()?.am_conn_core.full_send(
                request,
                self.o_a_rsmd.clone(),
                Some(self.a_descriptors.clone()),
                &mut None,
            )?;

            if let Some(Argument::WriteLobReply(wlr)) =
                main_reply.extract_first_arg_of_type(PartKind::WriteLobReply)
            {
                let locator_ids = wlr.into_locator_ids();
                if locator_ids.len() != readers.len() {
                    return Err(HdbError::Usage(format!(
                        "The number of provided readers ({}) does not match \
                         the number of required readers ({})",
                        readers.len(),
                        locator_ids.len()
                    )));
                }
                for (locator_id, (reader, type_id)) in locator_ids.into_iter().zip(readers) {
                    debug!("writing content to locator with id {:?}", locator_id);
                    if let HdbValue::LOBSTREAM(Some(reader)) = reader {
                        let mut reader = reader.lock().unwrap();
                        let mut writer = LobWriter::new(
                            locator_id,
                            type_id,
                            self.am_ps_core.lock()?.am_conn_core.clone(),
                        )?;
                        std::io::copy(&mut *reader, &mut writer)?;
                        writer.flush()?;
                    }
                }
            }
            main_reply.into_hdbresponse(
                &mut (self.am_ps_core.lock()?.am_conn_core),
                Some(&mut self.server_usage),
            )
        } else {
            self.execute_parameter_rows(None)
        }
    }

    /// Converts the input into a row of parameters and adds it to the batch of this
    /// `PreparedStatement`, if it is consistent with the metadata.
    pub fn add_batch<T: serde::ser::Serialize>(&mut self, input: &T) -> HdbResult<()> {
        trace!("PreparedStatement::add_batch()");
        if self.a_descriptors.has_in() {
            self.batch.push(input, &self.a_descriptors)?;
            return Ok(());
        }
        Err(HdbError::Serialization(
            SerializationError::StructuralMismatch("no metadata in add_batch()"),
        ))
    }

    /// Consumes the input as a row of parameters for the batch.
    ///
    /// Useful mainly for generic code.
    /// In most cases [`add_batch()`](struct.PreparedStatement.html#method.add_batch)
    /// is more convenient.
    /// Note that LOB streaming can not be combined with using the batch.
    pub fn add_row_to_batch(&mut self, hdb_values: Vec<HdbValue<'static>>) -> HdbResult<()> {
        trace!("PreparedStatement::add_row_to_batch()");
        if self.a_descriptors.has_in() {
            self.batch
                .push_hdb_values(hdb_values, &self.a_descriptors)?;
            return Ok(());
        }
        Err(HdbError::Serialization(
            SerializationError::StructuralMismatch("no metadata in add_row_to_batch()"),
        ))
    }

    /// Executes the statement with the collected batch, and clears the batch.
    ///
    /// Does nothing and returns with an error, if the statement needs input and no batch exists.
    /// If the statement does not need input and the batch is empty,
    /// a single execution is triggered.
    pub fn execute_batch(&mut self) -> HdbResult<HdbResponse> {
        if self.batch.is_empty() && self.a_descriptors.has_in() {
            return Err(HdbError::Usage(
                "The batch is empty and cannot be executed".to_string(),
            ));
        }
        let mut rows2 = ParameterRows::new();
        mem::swap(&mut self.batch, &mut rows2);
        self.execute_parameter_rows(Some(rows2))
    }

    /// Descriptors of all parameters of the prepared statement (in, out, inout).
    pub fn parameter_descriptors(&self) -> Arc<ParameterDescriptors> {
        Arc::clone(&self.a_descriptors)
    }

    fn execute_parameter_rows(&mut self, o_rows: Option<ParameterRows>) -> HdbResult<HdbResponse> {
        trace!("PreparedStatement::execute_parameter_rows()");
        let mut request = Request::new(RequestType::Execute, HOLD_CURSORS_OVER_COMMIT);
        request.push(Part::new(
            PartKind::StatementId,
            Argument::StatementId(self.am_ps_core.lock()?.statement_id),
        ));
        if let Some(rows) = o_rows {
            request.push(Part::new(PartKind::Parameters, Argument::Parameters(rows)));
        }

        let reply = self.am_ps_core.lock()?.am_conn_core.full_send(
            request,
            self.o_a_rsmd.clone(),
            Some(self.a_descriptors.clone()),
            &mut None,
        )?;
        let mut response = reply.into_hdbresponse(
            &mut (self.am_ps_core.lock()?.am_conn_core),
            Some(&mut self.server_usage),
        )?;
        response.inject_statement_id(Arc::clone(&self.am_ps_core));
        Ok(response)
    }

    /// Provides information about the the server-side resource consumption that
    /// is related to this `PreparedStatement` object.
    pub fn server_usage(&self) -> ServerUsage {
        self.server_usage
    }

    // Prepare a statement.
    pub(crate) fn try_new(
        mut am_conn_core: AmConnCore,
        stmt: &str,
    ) -> HdbResult<PreparedStatement> {
        let mut request = Request::new(RequestType::Prepare, HOLD_CURSORS_OVER_COMMIT);
        request.push(Part::new(PartKind::Command, Argument::Command(stmt)));

        let mut reply = am_conn_core.send(request)?;

        // ParameterMetadata, ResultSetMetadata
        // StatementContext, StatementId,
        // TableLocation, TransactionFlags,
        let mut o_table_location: Option<Vec<i32>> = None;
        let mut o_stmt_id: Option<u64> = None;
        let mut a_descriptors: Arc<ParameterDescriptors> = Arc::new(ParameterDescriptors::new());
        let mut o_a_rsmd: Option<Arc<ResultSetMetadata>> = None;
        let mut server_usage: ServerUsage = Default::default();

        while !reply.parts.is_empty() {
            match reply.parts.pop_arg() {
                Some(Argument::ParameterMetadata(descriptors)) => {
                    a_descriptors = Arc::new(descriptors);
                }
                Some(Argument::StatementId(id)) => {
                    o_stmt_id = Some(id);
                }
                Some(Argument::TransactionFlags(ta_flags)) => {
                    let mut guard = am_conn_core.lock()?;
                    (*guard).evaluate_ta_flags(ta_flags)?;
                }
                Some(Argument::TableLocation(vec_i)) => {
                    o_table_location = Some(vec_i);
                }
                Some(Argument::ResultSetMetadata(rs_md)) => {
                    o_a_rsmd = Some(Arc::new(rs_md));
                }

                Some(Argument::StatementContext(ref stmt_ctx)) => {
                    let mut guard = am_conn_core.lock()?;
                    (*guard).evaluate_statement_context(stmt_ctx)?;
                    server_usage.update(
                        stmt_ctx.server_processing_time(),
                        stmt_ctx.server_cpu_time(),
                        stmt_ctx.server_memory_usage(),
                    );
                }
                x => warn!("prepare(): Unexpected reply part found {:?}", x),
            }
        }

        let statement_id = match o_stmt_id {
            Some(id) => id,
            None => {
                return Err(HdbError::Impl(
                    "PreparedStatement needs a StatementId".to_owned(),
                ));
            }
        };

        debug!(
            "PreparedStatement created with parameter descriptors = {:?}",
            a_descriptors
        );

        let am_ps_core = Arc::new(Mutex::new(PreparedStatementCore {
            am_conn_core,
            statement_id,
        }));
        Ok(PreparedStatement {
            am_ps_core,
            server_usage,
            batch: ParameterRows::new(),
            a_descriptors,
            o_a_rsmd,
            _o_table_location: o_table_location,
        })
    }
}

impl Drop for PreparedStatementCore {
    /// Frees all server-side ressources that belong to this prepared statement.
    fn drop(&mut self) {
        let mut request = Request::new(RequestType::DropStatementId, 0);
        request.push(Part::new(
            PartKind::StatementId,
            Argument::StatementId(self.statement_id),
        ));
        if let Ok(mut reply) = self.am_conn_core.send(request) {
            reply.parts.pop_arg_if_kind(PartKind::StatementContext);
        }
    }
}
