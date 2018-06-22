use serde;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde::Deserialize;
use serde_json;
use std::fmt;
use std::io;
use std::marker::Sized;

use jsonrpc_core::{self as jsonrpc, Id};

use server::transport::{LspTransport, Transport};

pub struct JsonRpcServer<T: Transport> {
    transport: T,
}

impl<T: Transport> JsonRpcServer<T> {
    fn parse_message<'a>(
        packet: &str,
    ) -> Result<Message<JsonResponseHandle<'a, T>>, jsonrpc::Failure> {
        let msg: serde_json::Value = serde_json::from_str(&packet).unwrap();
        let id = msg.get("id").map_or(Id::Null, |id| {
            // TODO: do not unwrap
            serde_json::from_value(id.to_owned()).unwrap()
        });
        let method = match msg.get("method") {
            Some(method) => method,
            None => {
                return Err(jsonrpc::Failure {
                    jsonrpc: Some(jsonrpc::types::version::Version::V2),
                    id: id,
                    error: jsonrpc::Error::invalid_request(),
                })
            }
        };
        let method = method.as_str().unwrap().to_owned();

        let params = match msg.get("params").map(|p| p.to_owned()) {
            Some(params @ serde_json::Value::Object(..))
            | Some(params @ serde_json::Value::Array(..)) => params,
            // Null as input value is not allowed by JSON-RPC 2.0,
            // but including it for robustness
            Some(serde_json::Value::Null) | None => serde_json::Value::Null,
            _ => panic!("test"),
        };

        return Ok(Message::Notification(RawNotification {
            method: method,
            params: params,
        }));
    }

    pub fn read_message<'a>(&'a mut self) -> Result<Message<JsonResponseHandle<'a, T>>, io::Error> {
        loop {
            let packet = self.transport.receive_packet()?;

            match Self::parse_message(&packet) {
                Ok(message) => return Ok(message),
                Err(failure) => {
                    // TODO: send this failure back to client.
                    continue;
                }
            }
        }
    }
}

pub struct JsonResponseHandle<'a, T: Transport + 'a> {
    id: i32,
    server: &'a JsonRpcServer<T>,
}

impl<'a, T: Transport> ResponseHandle for JsonResponseHandle<'a, T> {
    fn success<R: ::serde::Serialize + fmt::Debug>(self, result: R) {}
    fn failure(self, error: jsonrpc::Error) {}
}

// Explanations:
// - This type has to be `Sized`, because in both success and failure we take `self`, which
//   requires the type to be sized.
pub trait ResponseHandle
where
    Self: Sized,
{
    fn success<R: ::serde::Serialize + fmt::Debug>(self, result: R) {}
    fn failure(self, error: jsonrpc::Error) {}
}

/*
pub struct TypedResponseHandle<R: ::serde::Serialize + fmt::Debug> {
    handle: ResponseHandle,
}

impl<R> TypedResponseHandle<R: ::serde::Serialize + fmt::Debug> {
    fn success(self, result: R) {
        self.handle.success(result)
    }
    fn failure(self, error: jsonrpc::Error) {
        self.handle.failure(error)
    }
}*/

pub trait Request {
    type Params;
    type Result;
    const METHOD: &'static str;
}

#[derive(Debug, Deserialize, Serialize)]
struct HoverParams {}
#[derive(Debug, Deserialize, Serialize)]
struct HoverResult {}
struct HoverRequest {}

impl Request for HoverRequest {
    type Params = HoverParams;
    type Result = HoverResult;
    const METHOD: &'static str = "hover";
}

#[derive(Debug)]
pub struct RawRequest<R: ResponseHandle> {
    method: String,
    params: serde_json::Value,
    response: R,
}

#[derive(Debug)]
pub struct RawNotification {
    method: String,
    params: serde_json::Value,
}

// Incoming message (request or notification).
#[derive(Debug)]
pub enum Message<R: ResponseHandle> {
    Request(RawRequest<R>),
    Notification(RawNotification),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeTransport {
        message: String,
    }
    impl Transport for FakeTransport {
        fn receive_packet(&mut self) -> Result<String, io::Error> {
            return Ok(self.message.to_owned());
        }
    }

    #[test]
    fn reads_notification() {
        let mut server = JsonRpcServer {
            transport: FakeTransport {
                message: json!({
                    "method": "hover",
                    "params": {
                        "key": "value"
                    }
                }).to_string(),
            },
        };
        let message = server
            .read_message()
            .expect("Notification should be returned from valid notification packet");

        let raw_notification = match message {
            Message::Notification(n) => n,
            _ => panic!("Expected notification, got "),
        };

        assert_eq!(raw_notification.method, "hover");
        assert_eq!(raw_notification.params, json!({"key": "value"}));
    }
}
