// Copyright 2017 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::error::Error;
use std::io::{self, BufRead, Write};

/// Sends given packet to a client.
/// fn send_packet(&mut self, packet: &str) -> Result<(), std::io::Error>;

/// A transport mechanism used for communication between client and server.
pub trait Transport {
    /// Reads a next packet from a client.
    fn receive_packet(&mut self) -> Result<String, io::Error>;
}

pub fn read_lsp_packet<R: BufRead>(input:&mut R) -> Result<String, io::Error> {
        let mut packet_size: Option<usize> = None;
        // Read headers
        loop {
            let mut buf = String::new();
            let read_bytes = input.read_line(&mut buf)?;
            // If 0 bytes were read, it means we reached EOF.
            if read_bytes == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, ""));
            }
            if buf == "\r\n" {
                break;
            }
            let header = match LspHeader::parse_from_line(&buf) {
                Ok(header) => header,
                Err(msg) => return Err(io::Error::new(io::ErrorKind::InvalidData, msg)),
            };

            // We are currently interested only in content-length header, and we ignore the rest.
            if header.key.to_lowercase() != "content-length" {
                continue;
            }
            packet_size = match usize::from_str_radix(header.value, 10) {
                Ok(size) => Some(size),
                Err(parse_error) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "Value of Content-Length header is invalid number: {}",
                            parse_error.description()
                        ),
                    ))
                }
            };
        }

        let size = match packet_size {
            Some(size) => size,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Content-Length header is missing",
                ))
            }
        };

        let mut content = vec![0; size];
        input.read_exact(&mut content)?;
        String::from_utf8(content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Content of a packet is not a valid utf8: {}",
                    e.description()
                ),
            )
        })
}

pub struct LspStdTransport {
}

impl Transport for LspStdTransport {
    fn receive_packet(&mut self) -> Result<String, io::Error> {
        let stdin = io::stdin();
        let mut locked = stdin.lock();
        read_lsp_packet(&mut locked)
    }
}

/// A Transport implementation that uses Language Server Protocol to transport packets between
/// client and server.
pub struct LspTransport<R: BufRead> {
    input: R,
}

impl<R: BufRead> LspTransport<R> {
    pub fn new(input: R) -> Self {
        return LspTransport{input: input}
    }
}

impl<R: BufRead> Transport for LspTransport<R> {
    // Returns the next packet.
    //
    // Returns error if we failed to read a packet, either because of format issue or because
    // stream ended.
    //
    // If error is returned, this method should not longer be called.
    fn receive_packet(&mut self) -> Result<String, io::Error> {
        read_lsp_packet(&mut self.input)
    }

    /*
    fn send_packet(&mut self, packet: &str) -> Result<(), std::io::Error> {
        return Ok(());
    }
    */
}

struct LspHeader<'a> {
    key: &'a str,
    value: &'a str,
}

impl<'a> LspHeader<'a> {
    fn parse_from_line(line: &'a str) -> Result<LspHeader<'a>, String> {
        let split: Vec<&str> = line.splitn(2, ": ").collect();
        if split.len() != 2 {
            return Err(format!("Malformed LSP header: '{}'", line));
        }
        return Ok(LspHeader {
            key: split[0].trim(),
            value: split[1].trim(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_packet_returns_packet_from_valid_lsr_input() {
        let cursor = io::Cursor::new("Content-Length: 7\r\n\r\nMessage");
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect("Reading a packet from valid input should succeed");

        assert_eq!(packet, "Message")
    }

    #[test]
    fn receive_packet_fails_on_empty_input() {
        let cursor = io::Cursor::new("");
        let mut transport = LspTransport { input: cursor };

        transport
            .receive_packet()
            .expect_err("Empty input should cause failure");
    }

    #[test]
    fn receive_packet_returns_packet_from_input_with_multiple_headers() {
        let cursor =
            io::Cursor::new("Content-Encoding: utf8\r\nContent-Length: 12\r\n\r\nSome Message");
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect("Reading a packet from valid input should succeed");

        assert_eq!(packet, "Some Message")
    }

    #[test]
    fn receive_packet_returns_packet_from_input_with_strange_headers() {
        let cursor = io::Cursor::new(
            "Unknown-Header: value :value\r\nContent-Length: 12\r\n\r\nSome Message",
        );
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect("Reading a packet from valid input should succeed");

        assert_eq!(packet, "Some Message")
    }

    #[test]
    fn receive_packet_fails_when_length_header_is_missing() {
        let cursor = io::Cursor::new("Content-Encoding: utf8\r\n\r\nSome Message");
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect_err("Reading a packet with no length header should fail.");
    }

    #[test]
    fn receive_packet_fails_when_header_line_is_invalid() {
        let cursor = io::Cursor::new("Invalid-Header\r\nContent-Length: 12\r\n\r\nSome Message");
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect_err("Reading a packet with invalid header should fail.");
    }

    #[test]
    fn receive_packet_fails_when_length_is_not_numeric() {
        let cursor = io::Cursor::new("Content-Length: abcd\r\n\r\nMessage");
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect_err("Reading a packet with no length header should fail.");
    }

    #[test]
    fn receive_packet_fails_when_length_is_too_large_integer() {
        let cursor = io::Cursor::new("Content-Length: 1000000000000000000000\r\n\r\nMessage");
        let mut transport = LspTransport { input: cursor };

        let packet = transport
            .receive_packet()
            .expect_err("Reading a packet with no length header should fail.");
    }

    #[test]
    fn receive_packet_fails_when_content_is_not_valid_utf8() {
        let cursor = io::Cursor::new(b"Content-Length: 7\r\n\r\n\x82\xe6\x82\xa8\x82\xb1\x82");
        let mut transport = LspTransport { input: cursor };

        let packet = transport.receive_packet().expect_err(
            "Reading a packet with content containing invalid utf8 sequences should fail.",
        );
    }
}
