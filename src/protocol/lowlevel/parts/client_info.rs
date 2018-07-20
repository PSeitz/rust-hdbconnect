use super::typed_value::factory::parse_string;
use super::typed_value::{serialize_length_and_string, string_length};

use HdbResult;

use std::collections::HashMap;
use std::io;
use std::net::TcpStream;

#[derive(Debug)]
pub struct ClientInfo(HashMap<String, String>);

impl ClientInfo {
    pub fn serialize(&self, w: &mut io::Write) -> HdbResult<()> {
        for (key, value) in &self.0 {
            serialize_length_and_string(key, w)?;
            serialize_length_and_string(value, w)?;
        }
        Ok(())
    }

    pub fn size(&self) -> usize {
        let mut len = 0;
        for (key, value) in &self.0 {
            len += string_length(key) + string_length(value);
        }
        len
    }
    pub fn count(&self) -> usize {
        self.0.len()
    }

    pub fn parse_from_request(
        no_of_args: i32,
        rdr: &mut io::BufReader<TcpStream>,
    ) -> HdbResult<ClientInfo> {
        let mut map = HashMap::new();
        for _ in 0..no_of_args {
            let key = parse_string(rdr)?;
            let value = parse_string(rdr)?;
            map.insert(key, value);
        }
        Ok(ClientInfo(map))
    }

    #[allow(dead_code)] // FIXME (see info.txt)
    pub fn set(&mut self, key: &ClientInfoKey, value: String) {
        match *key {
            ClientInfoKey::Application => self.0.insert(String::from("APPLICATION"), value),
            ClientInfoKey::ApplicationVersion => {
                self.0.insert(String::from("APPLICATIONVERSION"), value)
            }
            ClientInfoKey::ApplicationSource => {
                self.0.insert(String::from("APPLICATIONSOURCE"), value)
            }
            ClientInfoKey::ApplicationUser => self.0.insert(String::from("APPLICATIONUSER"), value),
        };
    }
}

#[allow(dead_code)] // FIXME (see info.txt)
pub enum ClientInfoKey {
    Application,
    ApplicationVersion,
    ApplicationSource,
    ApplicationUser,
}
