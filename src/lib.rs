#[macro_use]
extern crate server;
extern crate bincode;

use bincode::{deserialize_from, serialize_into};
pub use server::logger;
pub use server::net::types;
use server::storage::ResultSet;
use std::fmt;
use std::io::{self, Write};
use std::net::{AddrParseError, TcpStream};
use std::str::FromStr;
use types::*;

use std::io::Read;

const PROTOCOL_VERSION: u8 = 1;

/// Client specific Error definition.
#[derive(Debug)]
pub enum Error {
    AddrParse(AddrParseError),
    Io(io::Error),
    UnexpectedPkg,
    Bincode(bincode::Error),
    Auth,
    Server(ClientErrMsg),
}

/// Implement display for description of Error
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        std::error::Error::description(self).fmt(f)
    }
}

/// Implement description for this Error enum
impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            &Error::AddrParse(_) => "wrong IPv4 address format",
            &Error::Io(_) => "IO error occured",
            &Error::UnexpectedPkg => "received unexpected package",
            &Error::Bincode(_) => "could not encode/decode send package",
            &Error::Auth => "could not authenticate user",
            &Error::Server(ref e) => &e.msg,
        }
    }
}

/// Implement the conversion from io::Error to Connection-Error
impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

/// Implement the conversion from AddrParseError to Connection-Error
impl From<AddrParseError> for Error {
    fn from(err: AddrParseError) -> Error {
        Error::AddrParse(err)
    }
}

/// Implement the conversion from (En/De)codingError to NetworkError
impl From<bincode::Error> for Error {
    fn from(err: bincode::Error) -> Error {
        Error::Bincode(err)
    }
}

/// Implement the conversion from ClientErrMsg to NetworkError
impl From<ClientErrMsg> for Error {
    fn from(err: ClientErrMsg) -> Error {
        Error::Server(err)
    }
}

/// Stores TCPConnection with a server. Contains IP, Port, Login data and
/// greeting from server.
pub struct Connection {
    ip: String,
    port: u16,
    tcp: TcpStream,
    greeting: Greeting,
    user_data: Login,
}

impl Connection {
    /// Establish connection to specified address and port.
    pub fn connect(
        addr: String,
        port: u16,
        usern: String,
        passwd: String,
    ) -> Result<Connection, Error> {
        // Parse IPv4 address from String
        let tmp_addr = match std::net::Ipv4Addr::from_str(&addr) {
            Ok(tmp_addr) => tmp_addr,
            Err(e) => return Err(e.into()),
        };

        // Establish Tcp connection
        let mut tmp_tcp = match TcpStream::connect((tmp_addr, port)) {
            Ok(tmp_tcp) => tmp_tcp,
            Err(e) => return Err(e.into()),
        };

        // Greeting message
        match receive(&mut tmp_tcp, PkgType::Greet) {
            Ok(_) => {}
            Err(e) => return Err(e),
        };
        let greet: Greeting = try!(deserialize_from(&mut tmp_tcp));

        // Login package
        let log = Login {
            username: usern,
            password: passwd,
        };
        match serialize_into(&mut tmp_tcp, &PkgType::Login) {
            Ok(_) => {}
            Err(e) => return Err(e.into()),
        }

        // Login data
        match serialize_into(&mut tmp_tcp, &log) {
            Ok(_) => {}
            Err(e) => return Err(e.into()),
        }

        // Get Login response - either user is authorized or unauthorized
        let status: PkgType = try!(deserialize_from(&mut tmp_tcp));
        match status {
            PkgType::AccGranted => Ok(Connection {
                ip: addr,
                port: port,
                tcp: tmp_tcp,
                greeting: greet,
                user_data: log,
            }),
            PkgType::AccDenied => Err(Error::Auth),
            _ => Err(Error::UnexpectedPkg),
        }
    }

    /// Send ping-command to server and receive Ok-package
    pub fn ping(&mut self) -> Result<(), Error> {
        match send_cmd(&mut self.tcp, Command::Ping, 1024) {
            Ok(_) => {}
            Err(e) => return Err(e),
        };
        match receive(&mut self.tcp, PkgType::Ok) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Send quit-command to server and receive Ok-package
    pub fn quit(&mut self) -> Result<(), Error> {
        match send_cmd(&mut self.tcp, Command::Quit, 1024) {
            Ok(_) => {}
            Err(e) => return Err(e),
        };
        match receive(&mut self.tcp, PkgType::Ok) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    // TODO: Return results (response-package)
    pub fn execute(&mut self, query: String) -> Result<DataSet, Error> {
        match send_cmd(&mut self.tcp, Command::Query(query), 1024) {
            Ok(_) => {}
            Err(e) => return Err(e),
        };
        match receive(&mut self.tcp, PkgType::Response) {
            Ok(_) => {
                let rows: ResultSet = try!(deserialize_from(&mut self.tcp));
                let dataset = preprocess(&rows);
                Ok(dataset)
            }
            Err(err) => Err(err),
        }
    }

    /// Return server version number.
    pub fn get_version(&self) -> u8 {
        self.greeting.protocol_version
    }

    /// Return server greeting message.
    pub fn get_message(&self) -> &str {
        &self.greeting.message
    }

    /// Return ip address for current connection.
    pub fn get_ip(&self) -> &str {
        &self.ip
    }

    /// Return port for current connection.
    pub fn get_port(&self) -> u16 {
        self.port
    }

    /// Return username used for current connection authentication.
    pub fn get_username(&self) -> &str {
        &self.user_data.username
    }
}

/// Return current library version.
#[allow(dead_code)]
fn get_lib_version() -> u8 {
    PROTOCOL_VERSION
}

/// Send command package with actual command, e.g. quit, ping, query.
fn send_cmd<W: Write>(mut s: &mut W, cmd: Command, _size: u64) -> Result<(), Error> {
    try!(serialize_into(&mut s, &PkgType::Command));
    try!(serialize_into(&mut s, &cmd));
    Ok(())
}

/// Match received packages to expected packages.
fn receive(s: &mut TcpStream, cmd: PkgType) -> Result<(), Error> {
    let status: PkgType = try!(deserialize_from(s.take(1024)));

    if status == PkgType::Error {
        let err: ClientErrMsg = try!(deserialize_from(s));
        return Err(Error::Server(err));
    }

    if status != cmd {
        match status {
            PkgType::Ok => {}
            PkgType::Response => {
                let _: ResultSet = try!(deserialize_from(s));
            }
            PkgType::Greet => {
                let _: Greeting = try!(deserialize_from(s));
            }
            _ => {}
        }
        return Err(Error::UnexpectedPkg);
    }
    Ok(())
}
