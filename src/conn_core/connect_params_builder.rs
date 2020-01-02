use crate::conn_core::connect_params::{ConnectParams, ServerCerts};
use crate::{HdbErrorKind, HdbResult};
use secstr::SecStr;
use std::env;
use url::Url;

/// A builder for `ConnectParams`.
///
/// # Example
///
/// ```
/// use hdbconnect::ConnectParams;
/// let connect_params = ConnectParams::builder()
///     .hostname("abcd123")
///     .port(2222)
///     .dbuser("MEIER")
///     .password("schlau")
///     .build()
///     .unwrap();
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConnectParamsBuilder {
    hostname: Option<String>,
    port: Option<u16>,
    dbuser: Option<String>,
    #[serde(skip)]
    password: Option<SecStr>,
    clientlocale: Option<String>,
    server_certs: Vec<ServerCerts>,
    options: Vec<(String, String)>,
}

impl ConnectParamsBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            hostname: None,
            port: None,
            dbuser: None,
            password: None,
            clientlocale: None,
            server_certs: Vec::<ServerCerts>::default(),
            options: vec![],
        }
    }

    /// Sets the hostname.
    pub fn hostname<H: AsRef<str>>(&mut self, hostname: H) -> &mut Self {
        self.hostname = Some(hostname.as_ref().to_owned());
        self
    }

    /// Sets the port.
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = Some(port);
        self
    }

    /// Sets the database user.
    pub fn dbuser<D: AsRef<str>>(&mut self, dbuser: D) -> &mut Self {
        self.dbuser = Some(dbuser.as_ref().to_owned());
        self
    }

    /// Sets the password.
    pub fn password<P: AsRef<str>>(&mut self, pw: P) -> &mut Self {
        self.password = Some(SecStr::new(pw.as_ref().as_bytes().to_vec()));
        self
    }

    /// Sets the client locale.
    pub fn clientlocale<P: AsRef<str>>(&mut self, cl: P) -> &mut Self {
        self.clientlocale = Some(cl.as_ref().to_owned());
        self
    }

    /// Sets the client locale from the value of the environment variable LANG
    pub fn clientlocale_from_env_lang(&mut self) -> &mut Self {
        self.clientlocale = match std::env::var("LANG") {
            Ok(l) => Some(l),
            Err(_) => None,
        };

        self
    }

    /// Makes the driver use TLS for the connection to the database.
    ///
    /// Requires that the server's certificate is provided with one of the
    /// enum variants of [`ServerCerts`](enum.ServerCerts.html).
    ///
    /// If needed, you can call this function multiple times with different `ServerCert` variants.
    ///
    /// Example:
    ///
    /// ```rust,no_run
    /// # use hdbconnect::{ConnectParams,ServerCerts};
    /// # let string_with_certificate = String::new();
    /// let mut conn_params = ConnectParams::builder()
    ///    // ...more settings required...
    ///    .tls_with(ServerCerts::Direct(string_with_certificate))
    ///    .build();
    /// ```
    pub fn tls_with(&mut self, server_certs: ServerCerts) -> &mut Self {
        self.server_certs.push(server_certs);
        self
    }

    /// Adds a runtime parameter.
    pub fn option(&mut self, name: &str, value: &str) -> &mut Self {
        self.options.push((name.to_string(), value.to_string()));
        self
    }

    /// Constructs a `ConnectParams` from the builder.
    pub fn build(&self) -> HdbResult<ConnectParams> {
        let host = match self.hostname {
            Some(ref s) => s.clone(),
            None => return Err(HdbErrorKind::Usage("hostname is missing").into()),
        };

        let addr = format!(
            "{}:{}",
            host,
            match self.port {
                Some(p) => p,
                None => return Err(HdbErrorKind::Usage("port is missing").into()),
            }
        );
        let dbuser = match self.dbuser {
            Some(ref s) => s.clone(),
            None => return Err(HdbErrorKind::Usage("dbuser is missing").into()),
        };
        let password = match self.password {
            Some(ref secstr) => secstr.clone(),
            None => return Err(HdbErrorKind::Usage("password is missing").into()),
        };

        Ok(ConnectParams::new(
            host,
            addr,
            dbuser,
            password,
            self.clientlocale.clone(),
            self.server_certs.clone(),
        ))
    }

    /// Create `ConnectParamsBuilder` from url
    pub fn from_url(url: &Url) -> HdbResult<Self> {
        let host: String = match url.host_str() {
            Some("") | None => return Err(HdbErrorKind::Usage("host is missing").into()),
            Some(host) => host.to_string(),
        };

        let port: u16 = match url.port() {
            Some(p) => p,
            None => return Err(HdbErrorKind::Usage("port is missing").into()),
        };

        let dbuser: String = match url.username() {
            "" => return Err(HdbErrorKind::Usage("dbuser is missing").into()),
            s => s.to_string(),
        };

        let password = match url.password() {
            None => return Err(HdbErrorKind::Usage("password is missing").into()),
            Some(s) => s.to_string(),
        };

        let use_tls = match url.scheme() {
            "hdbsql" => false,
            "hdbsqls" => true,
            _ => {
                return Err(HdbErrorKind::Usage(
                    "Unknown protocol, only 'hdbsql' and 'hdbsqls' are supported",
                )
                .into());
            }
        };

        let mut server_certs = Vec::<ServerCerts>::new();
        let mut clientlocale = None;

        for (name, value) in url.query_pairs() {
            match name.as_ref() {
                "client_locale" => clientlocale = Some(value.to_string()),
                "client_locale_from_env" => {
                    clientlocale = env::var("LANG").ok();
                }
                "tls_certificate_dir" => {
                    server_certs.push(ServerCerts::Directory(value.to_string()));
                }
                "tls_certificate_env" => {
                    server_certs.push(ServerCerts::Environment(value.to_string()));
                }
                "use_mozillas_root_certificates" => {
                    server_certs.push(ServerCerts::RootCertificates);
                }
                _ => log::warn!("option {} not supported", name),
            }
        }

        if use_tls && server_certs.is_empty() {
            return Err(HdbErrorKind::Usage(
                "protocol 'hdbsqls' requires certificates, but none are specified",
            )
            .into());
        }

        let mut builder = Self::new();
        builder.hostname(host);
        builder.dbuser(dbuser);
        builder.port(port);
        builder.password(password);

        if let Some(cl) = clientlocale {
            builder.clientlocale(cl);
        }

        for cert in server_certs {
            builder.tls_with(cert);
        }

        Ok(builder)
    }

    /// Returns the url for this connection
    pub fn to_url(&self) -> HdbResult<String> {
        if let Some(dbuser) = &self.dbuser {
            if let Some(hostname) = &self.hostname {
                if let Some(port) = &self.port {
                    return Ok(format!(
                        "{}://{}@{}:{}{}",
                        self.get_protocol_name(),
                        dbuser,
                        hostname,
                        port,
                        self.get_options_as_parameters()
                    ));
                }
            }
        }

        Err(HdbErrorKind::Usage("missing data. not possible to build url").into())
    }

    fn get_protocol_name(&self) -> &str {
        if self.server_certs.is_empty() {
            "hdbsql"
        } else {
            "hdbsqls"
        }
    }

    fn get_options_as_parameters(&self) -> String {
        let mut result = String::with_capacity(200);
        for (index, (key, value)) in self.options.iter().enumerate() {
            let prefix = if index == 0 { "?" } else { "&" };
            result.push_str(&format!("{}{}={}", prefix, key, value));
        }
        result
    }

    /// Getter
    pub fn get_hostname(&self) -> Option<&String> {
        self.hostname.as_ref()
    }

    /// Getter
    pub fn get_dbuser(&self) -> Option<&String> {
        self.dbuser.as_ref()
    }

    /// Getter
    pub fn get_password(&self) -> Option<&SecStr> {
        self.password.as_ref()
    }

    /// Getter
    pub fn get_port(&self) -> Option<u16> {
        self.port
    }

    /// Getter
    pub fn get_clientlocale(&self) -> Option<&String> {
        self.clientlocale.as_ref()
    }

    /// Getter
    pub fn get_server_certs(&self) -> &Vec<ServerCerts> {
        &self.server_certs
    }

    /// Getter
    pub fn get_options(&self) -> &Vec<(String, String)> {
        &self.options
    }
}

impl From<Url> for ConnectParamsBuilder {
    fn from(u: Url) -> Self {
        match Self::from_url(&u) {
            Ok(connect_params_builder) => connect_params_builder,
            Err(error) => {
                panic!(error);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::ConnectParamsBuilder;
    use super::ServerCerts;

    #[test]
    fn test_connect_params_builder() {
        {
            let params = ConnectParamsBuilder::new()
                .hostname("abcd123")
                .port(2222)
                .dbuser("MEIER")
                .password("schLau")
                .build()
                .unwrap();
            assert_eq!("MEIER", params.dbuser());
            assert_eq!(b"schLau", params.password().unsecure());
            assert_eq!("abcd123:2222", params.addr());
            assert_eq!(None, params.clientlocale());
            assert!(params.server_certs().is_empty());
        }
        {
            let mut builder = ConnectParamsBuilder::new();
            builder
                .hostname("abcd123")
                .port(2222)
                .dbuser("MEIER")
                .password("schLau")
                .clientlocale("CL1");
            builder.tls_with(crate::ServerCerts::Directory("TCD".to_string()));
            builder.tls_with(crate::ServerCerts::RootCertificates);

            let params = builder.build().unwrap();
            assert_eq!("MEIER", params.dbuser());
            assert_eq!(b"schLau", params.password().unsecure());
            assert_eq!(Some(&"CL1".to_string()), params.clientlocale());
            assert_eq!(
                ServerCerts::Directory("TCD".to_string()),
                *params.server_certs().get(0).unwrap()
            );
            assert_eq!(
                ServerCerts::RootCertificates,
                *params.server_certs().get(1).unwrap()
            );
        }
    }
}
