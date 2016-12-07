use protocol::lowlevel::util;
use super::PrtResult;
use super::typed_value::TypedValue;
use super::typed_value::size as typed_value_size;
use super::typed_value::serialize as typed_value_serialize;

use std::io;

/// A single row of parameters; batches can consist of many such rows
#[derive(Debug,Clone)]
pub struct ParameterRow {
    pub values: Vec<TypedValue>,
}
impl ParameterRow {
    pub fn new() -> ParameterRow {
        ParameterRow { values: Vec::<TypedValue>::new() }
    }
    pub fn push(&mut self, val: TypedValue) {
        self.values.push(val)
    }

    pub fn size(&self) -> PrtResult<usize> {
        let mut size = 0;
        for ref value in &(self.values) {
            size += try!(typed_value_size(value));
        }
        Ok(size)
    }

    pub fn serialize(&self, w: &mut io::Write) -> PrtResult<()> {
        let mut data_pos = 0_i32;  // FIXME or must it be 1?
        // serialize the values (LOBs only serialize their header, the data follow below)
        for ref value in &(self.values) {
            try!(typed_value_serialize(value, &mut data_pos, w));
        }

        // serialize LOB data
        for value in &(self.values) {
            match *value {
                TypedValue::BLOB(ref blob) |
                TypedValue::N_BLOB(Some(ref blob)) => {
                    try!(util::serialize_bytes(&try!(blob.ref_to_bytes()), w))
                }

                TypedValue::CLOB(ref clob) |
                TypedValue::N_CLOB(Some(ref clob)) |
                TypedValue::NCLOB(ref clob) |
                TypedValue::N_NCLOB(Some(ref clob)) => {
                    try!(util::serialize_bytes(try!(clob.ref_to_string()).as_bytes(), w))
                }
                _ => {}
            }
        }
        Ok(())
    }
}


/// A PARAMETERS part contains input parameters.
/// The argument count of the part defines how many rows of parameters are included.
#[derive(Clone,Debug)]
pub struct Parameters {
    rows: Vec<ParameterRow>,
}
impl Parameters {
    pub fn new(rows: Vec<ParameterRow>) -> Parameters {
        Parameters { rows: rows }
    }

    pub fn serialize(&self, w: &mut io::Write) -> PrtResult<()> {
        for ref row in &self.rows {
            try!(row.serialize(w));
        }
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    pub fn size(&self) -> PrtResult<usize> {
        let mut size = 0;
        for ref row in &self.rows {
            size += try!(row.size());
        }
        Ok(size)
    }
}
