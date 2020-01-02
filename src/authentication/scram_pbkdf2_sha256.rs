use super::authenticator::Authenticator;
use super::crypto_util::scram_pdkdf2_sha256;
use crate::protocol::parts::authfields::AuthFields;
use crate::{HdbError, HdbErrorKind, HdbResult};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use chrono::Local;
use failure::ResultExt;
use rand::{thread_rng, RngCore};
use secstr::SecStr;
use std::io::Write;

const CLIENT_PROOF_SIZE: u8 = 32;

pub struct ScramPbkdf2Sha256 {
    client_challenge: Vec<u8>,
    server_proof: Option<Vec<u8>>,
}
impl ScramPbkdf2Sha256 {
    pub fn boxed_authenticator() -> Box<dyn Authenticator> {
        let mut client_challenge = [0_u8; 64];
        let mut rng = thread_rng();
        rng.fill_bytes(&mut client_challenge);
        Box::new(Self {
            client_challenge: client_challenge.to_vec(),
            server_proof: None,
        })
    }
}
impl Authenticator for ScramPbkdf2Sha256 {
    fn name(&self) -> &str {
        "SCRAMPBKDF2SHA256"
    }

    fn name_as_bytes(&self) -> Vec<u8> {
        self.name().as_bytes().to_owned()
    }

    fn client_challenge(&self) -> &[u8] {
        &(self.client_challenge)
    }

    fn client_proof(&mut self, server_data: &[u8], password: &SecStr) -> HdbResult<Vec<u8>> {
        const CONTEXT_CLIENT_PROOF: &str = "ClientProof";
        let (salt, server_nonce, iterations) = parse_first_server_data(server_data)?;

        let start = Local::now();
        let (client_proof, server_proof) = scram_pdkdf2_sha256(
            &salt,
            &server_nonce,
            &self.client_challenge,
            password,
            iterations,
        );
        debug!(
            "pbkdf2 took {} µs",
            Local::now()
                .signed_duration_since(start)
                .num_microseconds()
                .unwrap_or(-1)
        );

        self.client_challenge.clear();
        self.server_proof = Some(server_proof);

        let mut buf = Vec::<u8>::with_capacity(3 + (CLIENT_PROOF_SIZE as usize));
        buf.write_u16::<BigEndian>(1_u16)
            .context(HdbErrorKind::Impl(CONTEXT_CLIENT_PROOF))?;
        buf.write_u8(CLIENT_PROOF_SIZE as u8)
            .context(HdbErrorKind::Impl(CONTEXT_CLIENT_PROOF))?;
        buf.write_all(&client_proof)
            .context(HdbErrorKind::Impl(CONTEXT_CLIENT_PROOF))?;

        Ok(buf)
    }

    fn verify_server(&self, server_data: &[u8]) -> HdbResult<()> {
        let srv_proof = AuthFields::parse(&mut std::io::Cursor::new(server_data))
            .context(HdbErrorKind::Database)?
            .pop()
            .ok_or_else(|| HdbError::imp("expected non-empty list of auth fields"))?;

        if let Some(ref s_p) = self.server_proof {
            if s_p as &[u8] == &srv_proof as &[u8] {
                return Ok(());
            }
        }
        let msg = "Server proof failed - \
                   this indicates a severe security issue with the server's identity!";
        warn!("{}", msg);
        Err(HdbErrorKind::Usage(msg).into())
    }
}

// `server_data` is again an AuthFields, contains salt, server_nonce, iterations
fn parse_first_server_data(server_data: &[u8]) -> HdbResult<(Vec<u8>, Vec<u8>, u32)> {
    let mut af = AuthFields::parse(&mut std::io::Cursor::new(server_data))
        .context(HdbErrorKind::Database)?;

    match (af.pop(), af.pop(), af.pop(), af.pop()) {
        (Some(it_bytes), Some(server_nonce), Some(salt), None) => {
            let iterations = std::io::Cursor::new(it_bytes)
                .read_u32::<BigEndian>()
                .context(HdbErrorKind::Database)?;
            if iterations < 15_000 {
                Err(HdbError::imp_detailed(format!(
                    "not enough iterations: {}",
                    iterations
                )))
            } else if salt.len() < 16 {
                Err(HdbError::imp_detailed(format!(
                    "too little salt: {}",
                    salt.len()
                )))
            } else {
                Ok((salt, server_nonce, iterations))
            }
        }
        (_, _, _, _) => Err(HdbError::imp("expected 3 auth fields")),
    }
}
