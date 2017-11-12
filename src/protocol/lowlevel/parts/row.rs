use serde;
use serde_db::de::{DbValue, DeserializableRow};
use std::fmt;
use std::mem;
use std::vec;
use std::sync::Arc;

use {HdbError, HdbResult};
use protocol::lowlevel::parts::resultset_metadata::ResultSetMetadata;
use protocol::lowlevel::parts::typed_value::TypedValue;
use types::{BLOB, CLOB};
use serde_db::de::ConversionError;

/// A generic implementation of a single line of a `ResultSet`.
#[derive(Clone, Debug)]
pub struct Row {
    metadata: Arc<ResultSetMetadata>,
    values: Vec<TypedValue>,
}

impl Row {
    /// Factory for row.
    pub fn new(metadata: Arc<ResultSetMetadata>, values: Vec<TypedValue>) -> Row {
        Row {
            metadata: metadata,
            values: values,
        }
    }

    /// Returns the length of the row.
    pub fn len(&self) -> usize {
        trace!("Row::len()");
        self.values.len()
    }

    /// Returns true if the row contains no value.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Removes and returns the last value.
    pub fn pop(&mut self) -> Option<TypedValue> {
        trace!("Row::pop()");
        self.values.pop()
    }

    /// Returns the name of the column at the specified index
    pub fn get_fieldname(&self, field_idx: usize) -> Option<&String> {
        trace!("Row::get_fieldname()");
        self.metadata.get_fieldname(field_idx)
    }

    /// Reverses the order of the values
    pub fn reverse_values(&mut self) {
        trace!("Row::reverse()");
        self.values.reverse()
    }

    /// Returns a clone of the ith value.
    pub fn cloned_value(&self, i: usize) -> HdbResult<TypedValue> {
        trace!("Row::cloned_value()");
        self.values.get(i)
            .cloned()
            .ok_or_else(|| HdbError::UsageError("element with index {} does not exist".to_owned()))
    }

    /// Pops and converts the last field into a plain rust value.
    pub fn pop_into<'de, T>(&mut self) -> Result<T, <Row as DeserializableRow>::E>
    where
        T: serde::de::Deserialize<'de>,
    {
        trace!("Row::pop_into()");
        Ok(DbValue::into_typed(DeserializableRow::pop(self).unwrap())?)
    }

    /// Swaps out a field and converts it into a plain rust value.
    pub fn field_into<'de, T>(&mut self, i: usize) -> HdbResult<T>
    where
        T: serde::de::Deserialize<'de>,
    {
        trace!("Row::field_into()");
        let mut tmp = TypedValue::NOTHING;
        mem::swap(&mut self.values[i], &mut tmp);
        Ok(DbValue::into_typed(tmp)?)
    }

    /// Swaps out a field and converts it into a CLOB.
    pub fn field_into_clob(&mut self, i: usize) -> HdbResult<CLOB> {
        trace!("Row::field_into_clob()");
        let mut tmp = TypedValue::NOTHING;
        mem::swap(&mut self.values[i], &mut tmp);

        match tmp {
            TypedValue::CLOB(clob) | TypedValue::N_CLOB(Some(clob)) => Ok(clob),
            tv => Err(HdbError::ConversionError(ConversionError::ValueType(
                format!("The value {:?} cannot be converted into a CLOB", tv),
            ))),
        }
    }

    /// Swaps out a field and converts it into a BLOB.
    pub fn field_into_blob(&mut self, i: usize) -> HdbResult<BLOB> {
        trace!("Row::field_into_blob()");
        let mut tmp = TypedValue::NOTHING;
        mem::swap(&mut self.values[i], &mut tmp);

        match tmp {
            TypedValue::BLOB(blob) | TypedValue::N_BLOB(Some(blob)) => Ok(blob),
            tv => Err(HdbError::ConversionError(ConversionError::ValueType(
                format!("The value {:?} cannot be converted into a BLOB", tv),
            ))),
        }
    }

    /// Converts the Row into a rust value.
    pub fn try_into<'de, T>(self) -> HdbResult<T>
    where
        T: serde::de::Deserialize<'de>,
    {
        trace!("Row::into_typed()");
        Ok(DeserializableRow::into_typed(self)?)
    }
}

impl IntoIterator for Row {
    type Item = TypedValue;
    type IntoIter = vec::IntoIter<TypedValue>;

    fn into_iter(self) -> Self::IntoIter {
        trace!("<Row as IntoIterator>::into_iter()");
        self.values.into_iter()
    }
}

impl fmt::Display for Row {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        for v in &self.values {
            fmt::Display::fmt(&v, fmt)?;
            write!(fmt, "")?;
        }
        Ok(())
    }
}
