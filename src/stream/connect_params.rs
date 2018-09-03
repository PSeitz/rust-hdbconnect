//! Connection parameters
use secstr::SecStr;
use std::env;
use std::fmt::Debug;
use std::mem;
use url::{Host, Url};
use {HdbError, HdbResult};

/// An immutable struct with all information necessary to open a new connection
/// to a HANA database.
///
/// An instance of `ConnectParams` can be created either programmatically with
/// the builder, or implicitly using the trait `IntoConnectParams` and its
/// implementations.
///
/// # Example
///
/// ```
/// use hdbconnect::IntoConnectParams;
/// let conn_params = "hdbsql://my_user:my_passwd@the_host:2222"
///     .into_connect_params()
///     .unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct ConnectParams {
    host: Host,
    port: u16,
    dbuser: String,
    password: SecStr,
    clientlocale: Option<String>,
    options: Vec<(String, String)>,
}

impl ConnectParams {
    /// Returns a new builder for ConnectParams.
    pub fn builder() -> ConnectParamsBuilder {
        ConnectParamsBuilder::new()
    }

    /// The target host.
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// The target port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The database user.
    pub fn dbuser(&self) -> &String {
        &self.dbuser
    }

    /// The password.
    pub fn password(&self) -> &SecStr {
        &self.password
    }

    /// The client locale.
    pub fn clientlocale(&self) -> &Option<String> {
        &self.clientlocale
    }

    /// Options to be passed to HANA.
    pub fn options(&self) -> &[(String, String)] {
        &self.options
    }
}

/// A trait implemented by types that can be converted into a `ConnectParams`.
pub trait IntoConnectParams {
    /// Converts the value of `self` into a `ConnectParams`.
    fn into_connect_params(self) -> HdbResult<ConnectParams>;
}

impl IntoConnectParams for ConnectParams {
    fn into_connect_params(self) -> HdbResult<ConnectParams> {
        Ok(self)
    }
}

impl<'a> IntoConnectParams for &'a str {
    fn into_connect_params(self) -> HdbResult<ConnectParams> {
        match Url::parse(self) {
            Ok(url) => url.into_connect_params(),
            Err(_) => Err(HdbError::Usage("url parse error".to_owned())),
        }
    }
}

impl IntoConnectParams for String {
    fn into_connect_params(self) -> HdbResult<ConnectParams> {
        self.as_str().into_connect_params()
    }
}

impl IntoConnectParams for Url {
    fn into_connect_params(self) -> HdbResult<ConnectParams> {
        let mut builder = ConnectParams::builder();

        if let Some(port) = self.port() {
            builder.port(port);
        }

        if !self.username().is_empty() {
            builder.dbuser(self.username());
        }
        if let Some(pass) = self.password() {
            builder.password(pass);
        }

        for (name, value) in self.query_pairs() {
            builder.option(&name, &value);
        }

        if let Some(host) = self.host() {
            builder.host(host.to_string());
        }

        builder.build()
    }
}

/// A builder for `ConnectParams`.
///
/// # Example
///
/// ```
/// use hdbconnect::ConnectParams;
/// let connect_params = ConnectParams::builder()
///     .host("abcd123")
///     .port(2222)
///     .dbuser("MEIER")
///     .password("schlau")
///     .build()
///     .unwrap();
/// ```
#[derive(Clone, Debug, Deserialize, Default)]
pub struct ConnectParamsBuilder {
    hostname: Option<String>,
    port: Option<u16>,
    dbuser: Option<String>,
    password: Option<String>,
    clientlocale: Option<String>,
    options: Vec<(String, String)>,
}

impl ConnectParamsBuilder {
    /// Creates a new builder.
    pub fn new() -> ConnectParamsBuilder {
        ConnectParamsBuilder {
            hostname: None,
            port: None,
            dbuser: None,
            password: None,
            clientlocale: None,
            options: vec![],
        }
    }

    /// Sets the host.
    pub fn host<H: AsRef<str> + Debug>(&mut self, host: H) -> &mut ConnectParamsBuilder {
        info!("cpb.host: called with {:?}", host);
        self.hostname = Some(host.as_ref().to_owned());
        self
    }

    /// Sets the port.
    pub fn port(&mut self, port: u16) -> &mut ConnectParamsBuilder {
        self.port = Some(port);
        self
    }

    /// Sets the database user.
    pub fn dbuser<D: AsRef<str>>(&mut self, dbuser: D) -> &mut ConnectParamsBuilder {
        self.dbuser = Some(dbuser.as_ref().to_owned());
        self
    }

    /// Sets the password.
    pub fn password<P: AsRef<str>>(&mut self, pw: P) -> &mut ConnectParamsBuilder {
        self.password = Some(pw.as_ref().to_owned());
        self
    }

    /// Sets the client locale.
    pub fn clientlocale<P: AsRef<str>>(&mut self, cl: P) -> &mut ConnectParamsBuilder {
        self.clientlocale = Some(cl.as_ref().to_owned());
        self
    }

    /// Sets the client locale from the value of the environment variable LANG
    pub fn clientlocale_from_env_lang(&mut self) -> &mut ConnectParamsBuilder {
        self.clientlocale = match env::var("LANG") {
            Ok(l) => Some(l),
            Err(_) => None,
        };

        self
    }

    /// Adds a runtime parameter.
    pub fn option<'a>(&'a mut self, name: &str, value: &str) -> &'a mut ConnectParamsBuilder {
        self.options.push((name.to_string(), value.to_string()));
        self
    }

    /// Constructs a `ConnectParams` from the builder.
    pub fn build(&mut self) -> HdbResult<ConnectParams> {
        info!("ConnectParamsBuilder: {:?}", self);
        Ok(ConnectParams {
            host: match self.hostname {
                Some(ref s) => {
                    Host::parse(s).map_err(|_| HdbError::Usage(format!("bad host: {}", s)))?
                }
                None => return Err(HdbError::Usage("host is missing".to_owned())),
            },
            port: match self.port {
                Some(p) => p,
                None => return Err(HdbError::Usage("port is missing".to_owned())),
            },
            dbuser: match self.dbuser {
                Some(_) => self.dbuser.take().unwrap(),
                None => return Err(HdbError::Usage("dbuser is missing".to_owned())),
            },
            password: match self.password {
                Some(_) => SecStr::from(self.password.take().unwrap()),
                None => return Err(HdbError::Usage("password is missing".to_owned())),
            },
            clientlocale: match self.clientlocale {
                Some(_) => Some(self.clientlocale.take().unwrap()),
                None => None,
            },
            options: mem::replace(&mut self.options, vec![]),
        })
    }
}

/// A trait implemented by types that can be converted into a
/// `ConnectParamsBuilder`.
pub trait IntoConnectParamsBuilder {
    /// Consumes the builder and produces a `ConnectParams`.
    fn into_connect_params_builder(self) -> HdbResult<ConnectParamsBuilder>;
}

impl IntoConnectParamsBuilder for ConnectParamsBuilder {
    fn into_connect_params_builder(self) -> HdbResult<ConnectParamsBuilder> {
        Ok(self)
    }
}

impl<'a> IntoConnectParamsBuilder for &'a str {
    fn into_connect_params_builder(self) -> HdbResult<ConnectParamsBuilder> {
        match Url::parse(self) {
            Ok(url) => url.into_connect_params_builder(),
            Err(_) => Err(HdbError::Usage("url parse error".to_owned())),
        }
    }
}

impl IntoConnectParamsBuilder for String {
    fn into_connect_params_builder(self) -> HdbResult<ConnectParamsBuilder> {
        self.as_str().into_connect_params_builder()
    }
}

impl IntoConnectParamsBuilder for Url {
    fn into_connect_params_builder(self) -> HdbResult<ConnectParamsBuilder> {
        let mut builder = ConnectParams::builder();

        if let Some(port) = self.port() {
            builder.port(port);
        }

        if !self.username().is_empty() {
            builder.dbuser(self.username());
        }
        if let Some(pass) = self.password() {
            builder.password(pass);
        }

        for (name, value) in self.query_pairs() {
            builder.option(&name, &value);
        }

        if let Some(host) = self.host() {
            builder.host(host.to_string());
        }
        Ok(builder)
    }
}

#[cfg(test)]
mod tests {
    use stream::connect_params::{IntoConnectParams, IntoConnectParamsBuilder};
    use {ConnectParams, ConnectParamsBuilder};

    #[test]
    fn test_oneliner() {
        let connect_params: ConnectParams = ConnectParams::builder()
            .host("abcd123")
            .port(2222)
            .dbuser("MEIER")
            .password("schlau")
            .build()
            .unwrap();
        assert_eq!("abcd123", connect_params.host().to_string());
        assert_eq!("MEIER", connect_params.dbuser());
        assert_eq!(2222, connect_params.port());
    }

    #[test]
    fn test_reuse_builder() {
        let mut cp_builder: ConnectParamsBuilder = ConnectParams::builder();
        cp_builder.host("abcd123").port(2222);
        let params1: ConnectParams = cp_builder
            .dbuser("MEIER")
            .password("schlau")
            .build()
            .unwrap();
        let params2: ConnectParams = cp_builder
            .dbuser("HALLODRI")
            .password("kannnix")
            .build()
            .unwrap();

        assert_eq!("abcd123", params1.host().to_string());
        assert_eq!("abcd123", params2.host().to_string());
        assert_eq!("MEIER", params1.dbuser());
        assert_eq!(b"schlau", params1.password().unsecure());
        assert_eq!("HALLODRI", params2.dbuser());
        assert_eq!(b"kannnix", params2.password().unsecure());
    }

    #[test]
    fn test_params_from_url() {
        let params = "hdbsql://meier:schLau@abcd123:2222"
            .into_connect_params()
            .unwrap();

        assert_eq!("meier", params.dbuser());
        assert_eq!(b"schLau", params.password().unsecure());
        assert_eq!("abcd123", params.host().to_string());
        assert_eq!(2222, params.port());
    }
    #[test]
    fn test_builder_from_url() {
        let params = "hdbsql://meier:schLau@abcd123:2222"
            .into_connect_params_builder()
            .unwrap()
            .password("GanzArgSchlau")
            .build()
            .unwrap();

        assert_eq!("meier", params.dbuser());
        assert_eq!(b"GanzArgSchlau", params.password().unsecure());
        assert_eq!("abcd123", params.host().to_string());
        assert_eq!(2222, params.port());
    }
}
