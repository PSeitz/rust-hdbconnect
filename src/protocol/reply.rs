//! Since there is obviously no usecase for multiple segments in one request,
//! we model message and segment together.
//! But we differentiate explicitly between request messages and reply messages.
use super::argument::Argument;
use conn_core::AmConnCore;
use super::part::{Part, Parts};
use super::part_attributes::PartAttributes;
use super::partkind::PartKind;
use super::parts::parameter_descriptor::ParameterDescriptor;
use super::parts::resultset::factory as ResultSetFactory;
use super::parts::resultset::ResultSet;
use super::parts::resultset_metadata::ResultSetMetadata;
use super::parts::server_error::{ServerError, Severity};
use super::reply_type::ReplyType;
use super::util;
use byteorder::{LittleEndian, ReadBytesExt};
use hdb_response::factory as HdbResponseFactory;
use hdb_response::factory::InternalReturnValue;
use std::io;
use {HdbError, HdbResponse, HdbResult};

#[derive(Debug)]
pub struct Reply {
    session_id: i64,
    replytype: ReplyType,
    pub parts: Parts,
}
impl Reply {
    fn new(session_id: i64, replytype: ReplyType) -> Reply {
        Reply {
            session_id,
            replytype,
            parts: Parts::default(),
        }
    }

    pub fn session_id(&self) -> i64 {
        self.session_id
    }

    // Parse a reply from the stream, building a Reply object.
    //
    // * `ResultSetMetadata` need to be injected in case of execute calls of
    //    prepared statements
    // * `ResultSet` needs to be injected (and is extended and returned)
    //    in case of fetch requests
    #[allow(unknown_lints)]
    #[allow(let_and_return)]
    pub fn parse(
        o_rs_md: Option<&ResultSetMetadata>,
        o_par_md: Option<&Vec<ParameterDescriptor>>,
        o_rs: &mut Option<&mut ResultSet>,
        am_conn_core: &AmConnCore,
        expected_reply_type: Option<ReplyType>,
        skip: SkipLastSpace,
    ) -> HdbResult<Reply> {
        trace!("Reply::parse()");

        let mut conn_core = am_conn_core.lock()?;
        let rdr = &mut *((*conn_core).reader().borrow_mut());

        let reply = Reply::parse_impl(o_rs_md, o_par_md, o_rs, am_conn_core, rdr, skip)?;

        // Make sure that here (after parsing) the buffer is empty
        // The following only works with nightly, because `.buffer()`
        // is on its way, but not yet in stable (https://github.com/rust-lang/rust/pull/49139)
        // and needs additionally to activate line 26 in lib.rs
        #[cfg(feature = "check_buffer")]
        {
            use std::io::BufRead;

            let buf_len = {
                let buf = rdr.buffer();
                if !buf.is_empty() {
                    error!(
                        "Buffer is not empty after Reply::parse() \'{:?}\'",
                        buf.to_vec()
                    );
                } else {
                    info!("Buffer is empty");
                }
                buf.len()
            };
            rdr.consume(buf_len);
        }

        reply.assert_expected_reply_type(expected_reply_type)?;
        Ok(reply)
    }

    fn parse_impl(
        o_rs_md: Option<&ResultSetMetadata>,
        o_par_md: Option<&Vec<ParameterDescriptor>>,
        o_rs: &mut Option<&mut ResultSet>,
        am_conn_core: &AmConnCore,
        rdr: &mut io::BufRead,
        skip: SkipLastSpace,
    ) -> HdbResult<Reply> {
        let (no_of_parts, mut reply) = parse_message_and_sequence_header(rdr)?;
        trace!("Reply::parse(): parsed the header");

        for i in 0..no_of_parts {
            let (part, padsize) = Part::parse(
                &mut (reply.parts),
                Some(am_conn_core),
                o_rs_md,
                o_par_md,
                o_rs,
                rdr,
            )?;
            reply.push(part);

            if i < no_of_parts - 1 {
                util::skip_bytes(padsize, rdr)?;
            } else {
                match skip {
                    SkipLastSpace::Soft => util::dont_use_soft_consume_bytes(padsize, rdr)?,
                    SkipLastSpace::Hard => util::skip_bytes(padsize, rdr)?,
                    SkipLastSpace::No => {}
                }
            }
        }
        Ok(reply)
    }

    fn assert_expected_reply_type(&self, expected_reply_type: Option<ReplyType>) -> HdbResult<()> {
        match expected_reply_type {
            None => Ok(()), // we had no clear expectation
            Some(fc) => {
                if self.replytype.to_i16() == fc.to_i16() {
                    Ok(()) // we got what we expected
                } else {
                    Err(HdbError::Impl(format!(
                        "unexpected reply_type (function code) {:?}",
                        self.replytype
                    )))
                }
            }
        }
    }

    pub fn handle_db_error(&mut self, am_conn_core: &mut AmConnCore) -> HdbResult<()> {
        let mut conn_core = am_conn_core.lock()?;
        (*conn_core).warnings.clear();

        let err_code = PartKind::Error.to_i8();
        match (&self.parts)
            .into_iter()
            .position(|p| p.kind().to_i8() == err_code)
        {
            None => {
                // No error part found, reply evaluation happens elsewhere
                Ok(())
            }
            Some(_) => {
                // Error part found, but could contain warnings only
                let mut retval: HdbResult<()> = Ok(());
                self.parts.reverse(); // digest with pop
                while let Some(part) = self.parts.pop() {
                    let (kind, arg) = part.into_elements();
                    match arg {
                        Argument::StatementContext(ref stmt_ctx) => {
                            (*conn_core).evaluate_statement_context(stmt_ctx)?;
                        }
                        Argument::TransactionFlags(ref ta_flags) => {
                            (*conn_core).evaluate_ta_flags(ta_flags)?;
                        }
                        Argument::Error(vec) => {
                            // warnings are filtered out and added to conn_core
                            let mut errors: Vec<ServerError> = vec.into_iter()
                                .filter_map(|se| match se.severity() {
                                    Severity::Warning => {
                                        (*conn_core).warnings.push(se);
                                        None
                                    }
                                    _ => Some(se),
                                })
                                .collect();
                            if errors.len() == 1 {
                                retval = Err(HdbError::DbError(errors.remove(0)));
                            } else {
                                retval = Err(HdbError::MultipleDbErrors(errors));
                            }
                            debug!("Reply::handle_db_error(): {:?}", retval);
                        }
                        _ => warn!(
                            "Reply::handle_db_error(): ignoring unexpected part of kind {:?}",
                            kind
                        ),
                    }
                }
                retval
            }
        }
    }

    pub fn push(&mut self, part: Part) {
        self.parts.push(part);
    }

    pub fn into_hdbresponse(mut self, am_conn_core: &mut AmConnCore) -> HdbResult<HdbResponse> {
        // digest parts, collect InternalReturnValues
        let mut conn_core = am_conn_core.lock()?;
        let mut int_return_values = Vec::<InternalReturnValue>::new();
        self.parts.reverse(); // digest the last part first
        while let Some(part) = self.parts.pop() {
            let (kind, arg) = part.into_elements();
            debug!("Reply::into_hdbresponse(): found part of kind {:?}", kind);
            match arg {
                Argument::StatementContext(ref stmt_ctx) => {
                    (*conn_core).evaluate_statement_context(stmt_ctx)?;
                }
                Argument::TransactionFlags(ref ta_flags) => {
                    (*conn_core).evaluate_ta_flags(ta_flags)?;
                }

                Argument::OutputParameters(op) => {
                    int_return_values.push(InternalReturnValue::OutputParameters(op));
                }
                Argument::ResultSet(Some(rs)) => {
                    int_return_values.push(InternalReturnValue::ResultSet(rs));
                }
                Argument::ResultSetMetadata(rsm) => match self.parts.pop() {
                    Some(part) => match *part.arg() {
                        Argument::ResultSetId(rs_id) => {
                            let rs = ResultSetFactory::resultset_new(
                                am_conn_core,
                                PartAttributes::new(0b_0000_0100),
                                rs_id,
                                rsm,
                                None,
                            );
                            int_return_values.push(InternalReturnValue::ResultSet(rs));
                        }
                        _ => panic!("wrong Argument variant: ResultSetID expected"),
                    },
                    _ => panic!("Missing required part ResultSetID"),
                },
                Argument::RowsAffected(vra) => {
                    int_return_values.push(InternalReturnValue::AffectedRows(vra));
                }
                _ => warn!(
                    "Reply::into_hdbresponse(): ignoring unexpected part of kind {:?}",
                    kind
                ),
            }
        }

        // re-pack InternalReturnValues into appropriate HdbResponse
        trace!(
            "Reply::into_hdbresponse(): building HdbResponse for a reply of type {:?}",
            self.replytype
        );
        trace!(
            "The found InternalReturnValues are: {:?}",
            int_return_values
        );
        match self.replytype {
            ReplyType::Select |
            ReplyType::SelectForUpdate => HdbResponseFactory::resultset(int_return_values),

            
            ReplyType::Ddl |
            ReplyType::Commit |
            ReplyType::Rollback => HdbResponseFactory::success(int_return_values),

            ReplyType::Nil | 
            ReplyType::Insert |
            ReplyType::Update |
            ReplyType::Delete => HdbResponseFactory::rows_affected(int_return_values),

            ReplyType::DbProcedureCall |
            ReplyType::DbProcedureCallWithResult =>
                HdbResponseFactory::multiple_return_values(int_return_values),

            
            // ReplyTypes that are handled elsewhere and that should not go through this method:
            ReplyType::Connect | ReplyType::Fetch | ReplyType::ReadLob |
            ReplyType::CloseCursor | ReplyType::Disconnect |
            ReplyType::XAControl | ReplyType::XARecover |

            // FIXME: 2 ReplyTypes that occur only in not yet implemented calls:
            ReplyType::FindLob |
            ReplyType::WriteLob |

            // FIXME: 4 ReplyTypes where it is unclear when they occur and what to return:
            ReplyType::Explain |
            ReplyType::XaStart |
            ReplyType::XaJoin |
            ReplyType::XAPrepare => {
                let s = format!(
                    "unexpected reply type {:?} in send_and_get_hdbresponse(), \
                     with these internal return values: {:?}", 
                    self.replytype, int_return_values);
                error!("{}",s);
                error!("Reply: {:?}",self);
                Err(HdbError::impl_(s))
            },
        }
    }
}

#[derive(Clone, Copy)]
pub enum SkipLastSpace {
    Hard,
    Soft,
    No,
}

impl Drop for Reply {
    fn drop(&mut self) {
        for part in &self.parts {
            warn!(
                "reply is dropped, but not all parts were evaluated: part-kind = {:?}",
                part.kind()
            );
        }
    }
}

///
pub fn parse_message_and_sequence_header(
    rdr: &mut io::BufRead,
) -> HdbResult<(i16, Reply)> {
    // MESSAGE HEADER: 32 bytes
    let session_id: i64 = rdr.read_i64::<LittleEndian>()?; // I8
    let packet_seq_number: i32 = rdr.read_i32::<LittleEndian>()?; // I4
    let varpart_size: u32 = rdr.read_u32::<LittleEndian>()?; // UI4  not needed?
    let remaining_bufsize: u32 = rdr.read_u32::<LittleEndian>()?; // UI4  not needed?
    let no_of_segs = rdr.read_i16::<LittleEndian>()?; // I2
    assert_eq!(no_of_segs, 1);

    util::skip_bytes(10, rdr)?; // (I1 + B[9])

    // SEGMENT HEADER: 24 bytes
    rdr.read_i32::<LittleEndian>()?; // I4 seg_size
    rdr.read_i32::<LittleEndian>()?; // I4 seg_offset
    let no_of_parts: i16 = rdr.read_i16::<LittleEndian>()?; // I2
    rdr.read_i16::<LittleEndian>()?; // I2 seg_number
    let seg_kind = Kind::from_i8(rdr.read_i8()?)?; // I1

    trace!(
        "message and segment header: {{ packet_seq_number = {}, varpart_size = {}, \
         remaining_bufsize = {}, no_of_parts = {} }}",
        packet_seq_number,
        varpart_size,
        remaining_bufsize,
        no_of_parts
    );

    match seg_kind {
        Kind::Request => Err(HdbError::Usage("Cannot _parse_ a request".to_string())),
        Kind::Reply | Kind::Error => {
            util::skip_bytes(1, rdr)?; // I1 reserved2
            let reply_type = ReplyType::from_i16(rdr.read_i16::<LittleEndian>()?)?; // I2
            util::skip_bytes(8, rdr)?; // B[8] reserved3
            debug!(
                "Reply::parse(): got reply of type {:?} and seg_kind {:?} for session_id {}",
                reply_type, seg_kind, session_id
            );
            Ok((no_of_parts, Reply::new(session_id, reply_type)))
        }
    }
}

/// Specifies the layout of the remaining segment header structure
#[derive(Debug)]
enum Kind {
    Request,
    Reply,
    Error,
}
impl Kind {
    fn from_i8(val: i8) -> HdbResult<Kind> {
        match val {
            1 => Ok(Kind::Request),
            2 => Ok(Kind::Reply),
            5 => Ok(Kind::Error),
            _ => Err(HdbError::Impl(format!(
                "Invalid value for message::Kind::from_i8() detected: {}",
                val
            ))),
        }
    }
}
