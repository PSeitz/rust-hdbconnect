use crate::conn::{initial_request, AmConnCore, ConnectParams, SessionState, TcpClient};
use crate::protocol::part::Part;
use crate::protocol::partkind::PartKind;
use crate::protocol::parts::client_info::ClientInfo;
use crate::protocol::parts::connect_options::ConnectOptions;
use crate::protocol::parts::execution_result::ExecutionResult;
use crate::protocol::parts::parameter_descriptor::ParameterDescriptors;
use crate::protocol::parts::resultset::RsState;
use crate::protocol::parts::resultset_metadata::ResultSetMetadata;
use crate::protocol::parts::server_error::{ServerError, Severity};
use crate::protocol::parts::statement_context::StatementContext;
use crate::protocol::parts::topology::Topology;
use crate::protocol::parts::transactionflags::TransactionFlags;
use crate::protocol::parts::Parts;
use crate::protocol::reply::Reply;
use crate::protocol::request::Request;
use crate::protocol::server_usage::ServerUsage;
use crate::{HdbError, HdbResult};
use std::mem;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ConnectionCore {
    authenticated: bool,
    session_id: i64,
    client_info: ClientInfo,
    client_info_touched: bool,
    seq_number: i32,
    auto_commit: bool,
    server_usage: ServerUsage,
    fetch_size: u32,
    lob_read_length: u32,
    lob_write_length: usize,
    session_state: SessionState,
    statement_sequence: Option<i64>, // statement sequence within the transaction
    connect_options: ConnectOptions,
    topology: Option<Topology>,
    pub warnings: Vec<ServerError>,
    tcp_conn: TcpClient,
}

impl<'a> ConnectionCore {
    pub(crate) fn try_new(params: ConnectParams) -> HdbResult<Self> {
        let connect_options = ConnectOptions::for_server(params.clientlocale(), get_os_user());
        let mut tcp_conn = TcpClient::try_new(params)?;
        initial_request::send_and_receive(&mut tcp_conn)?;

        Ok(Self {
            authenticated: false,
            session_id: 0,
            seq_number: 0,
            auto_commit: true,
            server_usage: ServerUsage::default(),
            fetch_size: crate::DEFAULT_FETCH_SIZE,
            lob_read_length: crate::DEFAULT_LOB_READ_LENGTH,
            lob_write_length: crate::DEFAULT_LOB_WRITE_LENGTH,
            client_info: ClientInfo::default(),
            client_info_touched: false,
            session_state: SessionState::default(),
            statement_sequence: None,
            connect_options,
            topology: None,
            warnings: Vec::<ServerError>::new(),
            tcp_conn,
        })
    }

    pub(crate) fn connect_params(&self) -> &ConnectParams {
        match self.tcp_conn {
            TcpClient::SyncPlain(ref pc) => pc.connect_params(),
            TcpClient::SyncTls(ref sc) => sc.connect_params(),
            #[cfg(feature = "alpha_nonblocking")]
            TcpClient::SyncNonblockingTls(ref tc) => tc.connect_params(),
        }
    }

    pub(crate) fn connect_string(&self) -> String {
        format!("{}", self.connect_params())
    }

    pub(crate) fn set_application<S: AsRef<str>>(&mut self, application: S) -> HdbResult<()> {
        self.client_info.set_application(application);
        self.client_info_touched = true;
        Ok(())
    }

    pub(crate) fn set_application_version(&mut self, version: &str) -> HdbResult<()> {
        self.client_info.set_application_version(version);
        self.client_info_touched = true;
        Ok(())
    }

    pub(crate) fn set_application_source(&mut self, source: &str) -> HdbResult<()> {
        self.client_info.set_application_source(source);
        self.client_info_touched = true;
        Ok(())
    }

    pub(crate) fn set_application_user(&mut self, application_user: &str) -> HdbResult<()> {
        self.client_info.set_application_user(application_user);
        self.client_info_touched = true;
        Ok(())
    }

    pub(crate) fn is_client_info_touched(&self) -> bool {
        self.client_info_touched
    }
    pub(crate) fn get_client_info_for_sending(&mut self) -> ClientInfo {
        debug!("cloning client info for sending");
        self.client_info_touched = false;
        self.client_info.clone()
    }

    pub(crate) fn evaluate_statement_context(
        &mut self,
        stmt_ctx: &StatementContext,
    ) -> HdbResult<()> {
        trace!(
            "Received StatementContext with sequence_info = {:?}",
            stmt_ctx.statement_sequence_info()
        );
        self.set_statement_sequence(stmt_ctx.statement_sequence_info());
        self.server_usage.update(
            stmt_ctx.server_processing_time(),
            stmt_ctx.server_cpu_time(),
            stmt_ctx.server_memory_usage(),
        );
        // todo do not ignore the other content of StatementContext
        // StatementContextId::SchemaName => 3,
        // StatementContextId::FlagSet => 4,
        // StatementContextId::QueryTimeout => 5,
        // StatementContextId::ClientReconnectionWaitTimeout => 6,

        Ok(())
    }

    pub(crate) fn set_auto_commit(&mut self, ac: bool) {
        self.auto_commit = ac;
    }

    pub(crate) fn is_auto_commit(&self) -> bool {
        self.auto_commit
    }

    pub(crate) fn server_usage(&self) -> ServerUsage {
        self.server_usage
    }

    pub(crate) fn get_fetch_size(&self) -> u32 {
        self.fetch_size
    }

    pub(crate) fn set_fetch_size(&mut self, fetch_size: u32) {
        self.fetch_size = fetch_size;
    }

    pub(crate) fn get_lob_read_length(&self) -> u32 {
        self.lob_read_length
    }

    pub(crate) fn set_lob_read_length(&mut self, lob_read_length: u32) {
        self.lob_read_length = lob_read_length;
    }

    pub(crate) fn get_lob_write_length(&self) -> usize {
        self.lob_write_length
    }

    pub(crate) fn set_lob_write_length(&mut self, lob_write_length: usize) {
        self.lob_write_length = lob_write_length;
    }

    pub(crate) fn set_session_id(&mut self, session_id: i64) {
        if session_id != self.session_id {
            debug!(
                "ConnectionCore: setting session_id from {} to {}",
                self.session_id, session_id
            );
            self.session_id = session_id;
        }
    }

    pub(crate) fn set_topology(&mut self, topology: Topology) {
        self.topology = Some(topology);
    }

    pub(crate) fn dump_connect_options(&self) -> String {
        self.connect_options().to_string()
    }

    pub(crate) fn set_authenticated(&mut self) {
        self.authenticated = true;
    }

    pub(crate) fn statement_sequence(&self) -> &Option<i64> {
        &self.statement_sequence
    }

    fn set_statement_sequence(&mut self, statement_sequence: Option<i64>) {
        self.statement_sequence = statement_sequence;
    }

    pub(crate) fn session_id(&self) -> i64 {
        self.session_id
    }

    pub(crate) fn next_seq_number(&mut self) -> i32 {
        self.seq_number += 1;
        self.seq_number
    }
    pub(crate) fn last_seq_number(&self) -> i32 {
        self.seq_number
    }

    pub(crate) fn evaluate_ta_flags(&mut self, ta_flags: TransactionFlags) -> HdbResult<()> {
        self.session_state.update(ta_flags);
        if self.session_state.dead {
            Err(HdbError::SessionClosingTransactionError)
        } else {
            Ok(())
        }
    }

    pub(crate) fn pop_warnings(&mut self) -> HdbResult<Option<Vec<ServerError>>> {
        if self.warnings.is_empty() {
            Ok(None)
        } else {
            let mut v = Vec::<ServerError>::new();
            mem::swap(&mut v, &mut self.warnings);
            Ok(Some(v))
        }
    }

    pub(crate) fn connect_options(&self) -> &ConnectOptions {
        &self.connect_options
    }

    pub(crate) fn connect_options_mut(&mut self) -> &mut ConnectOptions {
        &mut self.connect_options
    }

    pub(crate) fn roundtrip_sync(
        &mut self,
        request: Request<'a>,
        am_conn_core: &AmConnCore,
        o_a_rsmd: Option<&Arc<ResultSetMetadata>>,
        o_a_descriptors: Option<&Arc<ParameterDescriptors>>,
        o_rs: &mut Option<&mut RsState>,
    ) -> HdbResult<Reply> {
        let session_id = self.session_id();
        let nsn = self.next_seq_number();
        let auto_commit = self.is_auto_commit();

        match self.tcp_conn {
            TcpClient::SyncPlain(ref mut pc) => {
                request.emit(session_id, nsn, auto_commit, o_a_descriptors, pc.writer())?;
            }
            TcpClient::SyncTls(ref mut tc) => {
                request.emit(session_id, nsn, auto_commit, o_a_descriptors, tc.writer())?;
            }
            #[cfg(feature = "alpha_nonblocking")]
            TcpClient::SyncNonblockingTls(ref mut tc) => {
                request.emit(session_id, nsn, auto_commit, o_a_descriptors, tc)?;
            }
        }

        let mut reply = match self.tcp_conn {
            TcpClient::SyncPlain(ref mut pc) => {
                let reader = pc.reader();
                Reply::parse(o_a_rsmd, o_a_descriptors, o_rs, Some(am_conn_core), reader)
            }
            TcpClient::SyncTls(ref mut tc) => {
                let reader = tc.reader();
                Reply::parse(o_a_rsmd, o_a_descriptors, o_rs, Some(am_conn_core), reader)
            }
            #[cfg(feature = "alpha_nonblocking")]
            TcpClient::SyncNonblockingTls(ref mut nbtc) => {
                Reply::parse(o_a_rsmd, o_a_descriptors, o_rs, Some(am_conn_core), nbtc)
            }
        }?;

        self.handle_db_error(&mut reply.parts)?;
        Ok(reply)
    }

    fn handle_db_error(&mut self, parts: &mut Parts<'static>) -> HdbResult<()> {
        self.warnings.clear();

        // Retrieve errors from returned parts
        let mut errors = {
            match parts.remove_first_of_kind(PartKind::Error) {
                None => {
                    // No error part found, regular reply evaluation happens elsewhere
                    return Ok(());
                }
                Some(Part::Error(server_errors)) => {
                    let (warnings, errors): (Vec<ServerError>, Vec<ServerError>) = server_errors
                        .into_iter()
                        .partition(|se| &Severity::Warning == se.severity());
                    self.warnings = warnings;
                    if errors.is_empty() {
                        // Only warnings, so return Ok(())
                        return Ok(());
                    } else {
                        errors
                    }
                }
                _ => unreachable!("129837938423"),
            }
        };

        // Evaluate the other parts that can come with an error part
        let mut o_rows_affected = None;
        parts.reverse(); // digest with pop
        while let Some(part) = parts.pop() {
            match part {
                Part::StatementContext(ref stmt_ctx) => {
                    self.evaluate_statement_context(stmt_ctx)?;
                }
                Part::TransactionFlags(ta_flags) => {
                    self.evaluate_ta_flags(ta_flags)?;
                }
                Part::ExecutionResult(vec) => {
                    o_rows_affected = Some(vec);
                }
                part => warn!(
                    "Reply::handle_db_error(): ignoring unexpected part of kind {:?}",
                    part.kind()
                ),
            }
        }

        match o_rows_affected {
            Some(rows_affected) => {
                // mix errors into rows_affected
                let mut err_iter = errors.into_iter();
                let mut rows_affected = rows_affected
                    .into_iter()
                    .map(|ra| match ra {
                        ExecutionResult::Failure(_) => ExecutionResult::Failure(err_iter.next()),
                        _ => ra,
                    })
                    .collect::<Vec<ExecutionResult>>();
                for e in err_iter {
                    warn!(
                        "Reply::handle_db_error(): \
                         found more errors than instances of ExecutionResult::Failure"
                    );
                    rows_affected.push(ExecutionResult::Failure(Some(e)));
                }
                Err(HdbError::ExecutionResults(rows_affected))
            }
            None => {
                if errors.len() == 1 {
                    Err(HdbError::from(errors.remove(0)))
                } else {
                    unreachable!("hopefully...")
                }
            }
        }
    }

    fn drop_impl(&mut self) -> std::io::Result<()> {
        debug!("Drop of ConnectionCore, session_id = {}", self.session_id);
        if self.authenticated {
            let request = Request::new_for_disconnect();
            let session_id = self.session_id();
            let nsn = self.next_seq_number();
            match self.tcp_conn {
                TcpClient::SyncPlain(ref mut pc) => {
                    request.emit(session_id, nsn, false, None, pc.writer())?;
                }
                TcpClient::SyncTls(ref mut tc) => {
                    request.emit(session_id, nsn, false, None, tc.writer())?;
                }
                #[cfg(feature = "alpha_nonblocking")]
                TcpClient::SyncNonblockingTls(ref mut nbtc) => {
                    request.emit(session_id, nsn, false, None, nbtc)?;
                }
            }
            trace!("Disconnect: request successfully sent");
        }
        Ok(())
    }
}

impl Drop for ConnectionCore {
    // try to send a disconnect to the database, ignore all errors
    fn drop(&mut self) {
        if let Err(e) = self.drop_impl() {
            warn!("Disconnect request failed with {:?}", e);
        }
    }
}

fn get_os_user() -> String {
    let os_user = username::get_user_name().unwrap_or_default();
    trace!("OS user: {}", os_user);
    os_user
}