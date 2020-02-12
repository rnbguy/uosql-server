//! The network api
//!
//! This module defines types and functions to read and write objects from
//! and to a TcpStream. However, the types in this module are types that are
//! passed to other methods and thus more "high level". They do not represent
//! the byte layout of objects in the protocol!
//!
//! # Protocol
//! All communication is send using TCP, which emulates a data stream. On top
//! of TCP, this database sends single packets.
//!
//! Every packet begins with a four byte `length` field that contains the
//! size of the packet in network byte order.
//!
//! ...
//!
pub mod types;

use std;
use std::fmt;
use std::io::{self, Read, Write};
// to encode and decode the structs to the given stream
use self::types::*;

use bincode::{deserialize_from, serialize_into};

use parse::parser::ParseError;
use storage::ResultSet;

const PROTOCOL_VERSION: u8 = 1;
const WELCOME_MSG: &'static str = "Welcome to the fabulous uoSQL database.";

/// Collection of possible errors while communicating with the client.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    UnexpectedPkg,
    UnknownCmd,
    Bincode(bincode::Error),
    UnEoq(ParseError),
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
            &Error::Io(_) => "IO error occured",
            &Error::UnexpectedPkg => "received unexpected package",
            &Error::UnknownCmd => "cannot interpret command: unknown",
            &Error::Bincode(_) => "could not encode/decode package",
            &Error::UnEoq(_) => "parsing error",
        }
    }
}

/// Implement the conversion from io::Error to NetworkError
impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

/// Implement the conversion from DecodingError to NetworkError
impl From<bincode::Error> for Error {
    fn from(err: bincode::Error) -> Error {
        Error::Bincode(err)
    }
}

/// Implement the conversion from ParseError to NetworkError
impl From<ParseError> for Error {
    fn from(err: ParseError) -> Error {
        Error::UnEoq(err)
    }
}

/// Write a welcome-message to the given server-client-stream.
pub fn do_handshake<W: Write + Read>(mut stream: &mut W) -> Result<(String, String), Error> {
    let greet = Greeting::make_greeting(PROTOCOL_VERSION, WELCOME_MSG.into());

    // send handshake packet to client
    try!(serialize_into(&mut stream, &PkgType::Greet));
    try!(serialize_into(&mut stream, &greet));

    // receive login data from client
    let login = read_login(stream);
    match login {
        Ok(sth) => Ok((sth.username, sth.password)),
        Err(msg) => Err(msg),
    }
}

/// Read the data from the response to the handshake,
/// username and password extracted and returned.
pub fn read_login<R: Read>(stream: R) -> Result<Login, Error> {
    // read package-type
    let status: PkgType = try!(deserialize_from(stream));

    match status {
        PkgType::Login =>
        // read the login data
        {
            // deserialize_from(stream).map_err(|e| e.into())
            Err(Error::UnexpectedPkg)
        }
        PkgType::Command => {
            // free the stream
            // let _: Command = try!(deserialize_from(stream));
            Err(Error::UnexpectedPkg)
        }
        _ => Err(Error::UnexpectedPkg),
    }
}

/// Read the sent bytes, extract the kind of command.
pub fn read_commands<R: Read>(stream: R) -> Result<Command, Error> {
    // read the first byte for code numeric value
    let status: PkgType = try!(deserialize_from(stream));

    match status {
        PkgType::Login => {
            // free the stream
            // let _: Login = try!(deserialize_from(stream));
            Err(Error::UnexpectedPkg)
        }
        PkgType::Command => {
            // deserialize_from(stream).map_err(|e| e.into());
            Err(Error::UnexpectedPkg)
        }
        _ => Err(Error::UnexpectedPkg),
    }
}

/// Send error package with given error code status.
pub fn send_error_package<W: Write>(mut stream: &mut W, err: ClientErrMsg) -> Result<(), Error> {
    try!(serialize_into(&mut stream, &PkgType::Error));
    try!(serialize_into(&mut stream, &err));
    Ok(())
}

/// Send information package only with package type information.
pub fn send_info_package<W: Write>(mut stream: &mut W, pkg: PkgType) -> Result<(), Error> {
    try!(serialize_into(&mut stream, &pkg));
    Ok(())
}

/// Send Result package as response to a query.
pub fn send_response_package<W: Write>(mut stream: &mut W, data: ResultSet) -> Result<(), Error> {
    try!(serialize_into(&mut stream, &PkgType::Response));
    try!(serialize_into(&mut stream, &data));
    Ok(())
}

// # Some information for the `net` working group:
//
// The net module is used by the `conn` module to receive commands from the
// client and to answer those commands.
//
// Your task is to:
// - Design the network protocol, which includes:
//   - What type of data is send when
//   - How to begin a connection
//   - The memory layout of packets
// - Create types that are more "high level" than the byte based network
//   types (see `Command` for example) and that can be used by other modules
// - Implement functions for every step of the connection (handshake,
//   receiving commands, sending answers, ...)
//

#[test]
pub fn test_send_ok_packet() {
    let mut vec = Vec::new();

    let res = send_info_package(&mut vec, PkgType::Ok);
    assert_eq!(res.is_ok(), true);
    assert_eq!(vec, vec![0, 0, 0, 4]);
}

#[test]
pub fn test_send_error_packet() {
    let mut vec = Vec::new(); // stream to write into
                              // could not encode/ send package
    let vec2 = vec![
        0, 0, 0, 3, // for error packet
        0, 2, // for kind of error
        0, 0, 0, 0, 0, 0, 0, 27, // for the size of the message string
        114, 101, 99, 101, 105, 118, 101, 100, 32, 117, 110, 101, 120, 112, 101, 99, 116, 101, 100,
        32, 112, 97, 99, 107, 97, 103, 101,
    ]; // string itself
    let err = Error::UnexpectedPkg;

    // test if the message is sent
    let res = send_error_package(&mut vec, err.into());
    assert_eq!(res.is_ok(), true);
    assert_eq!(vec, vec2);
}

#[test]
pub fn test_read_commands() {
    // test if the commands are correctly decoded
    use std::io::Cursor; // stream to read from
    let mut vec = Vec::new(); // stream to write into

    // write the command into the stream
    let _ = serialize_into(&mut vec, &PkgType::Command);
    let _ = serialize_into(&mut vec, &Command::Quit);

    // read the command from the stream for Command::Quit
    let mut command_res = read_commands(&mut Cursor::new(vec));
    assert_eq!(command_res.is_ok(), true);
    assert_eq!(command_res.unwrap(), Command::Quit);

    let mut vec2 = Vec::new();
    // write the command into the stream
    let _ = serialize_into(&mut vec2, &PkgType::Command);
    let _ = serialize_into(&mut vec2, &Command::Query("select".into()));

    // read the command from the stream for Command::Query("select")
    command_res = read_commands(&mut Cursor::new(vec2));
    assert_eq!(command_res.is_ok(), true);
    assert_eq!(command_res.unwrap(), Command::Query("select".into()));
}

#[test]
pub fn testlogin() {
    use std::io::Cursor; // stream to read from
    let mut vec = Vec::new(); // stream to write into

    // original struct
    let login = Login {
        username: "elena".into(),
        password: "prakt".into(),
    };
    let _ = serialize_into(&mut vec, &PkgType::Login);
    let _ = serialize_into(&mut vec, &login);

    let login_res = read_login(&mut Cursor::new(vec)).unwrap();

    // test for equality
    assert_eq!(login_res.username, "elena");
    assert_eq!(login_res.password, "prakt");
}
