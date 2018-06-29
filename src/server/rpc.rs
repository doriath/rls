use serde;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde::Deserialize;
use serde_json;
use std::fmt;
use std::io;
use std::marker::Sized;

use jsonrpc_core::{self as jsonrpc, Id};

pub struct JsonRpcServer<T: Transport> {
    transport: T,
}

impl<T: Transport> JsonRpcServer<T> {
    fn parse_message<'a>(
        &'a self,
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
            // TODO: do not panic
            _ => panic!("test"),
        };

        match id {
            Id::Null => Ok(Message::Notification(RawNotification {
                method: method,
                params: params,
            })),
            _ => Ok(Message::Request(RawRequest {
                method: method,
                params: params,
                response: JsonResponseHandle {
                    id: 123,
                    server: self,
                },
            })),
        }
    }

    pub fn read_message<'a>(&'a self) -> Result<Message<JsonResponseHandle<'a, T>>, io::Error> {
        loop {
            let packet = self.transport.receive_packet()?;

            match self.parse_message(&packet) {
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
    fn success(self, result: serde_json::Value) {
        // TODO: unwrap
        self.server
            .transport
            .send_packet(json!({ "result": result }).to_string())
            .unwrap();
    }
    fn failure(self, error: jsonrpc::Error) {
        self.server
            .transport
            .send_packet(json!({ "error": error }).to_string())
            .unwrap();
    }
}

// Explanations:
// - This type has to be `Sized`, because in both success and failure we take `self`, which
//   requires the type to be sized.
pub trait ResponseHandle
where
    Self: Sized,
{
    fn success(self, result: serde_json::Value);
    fn failure(self, error: jsonrpc::Error);
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

/// A transport mechanism used for communication between client and server.
pub trait Transport {
    /// Reads a next packet from a client.
    fn receive_packet(&self) -> Result<String, io::Error>;
    fn send_packet(&self, packet: String) -> Result<(), io::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver, Sender};

    struct FakeTransport {
        receiver: Receiver<String>,
        sender: Sender<String>,
    }
    impl Transport for FakeTransport {
        fn receive_packet(&self) -> Result<String, io::Error> {
            return Ok(self.receiver.recv().unwrap());
        }
        fn send_packet(&self, packet: String) -> Result<(), io::Error> {
            return Ok(self.sender.send(packet).unwrap());
        }
    }

    #[test]
    fn read_message_returns_notification_when_message_is_missing_id() {
        // TODO refactor
        let (sender1, receiver1) = channel::<String>();
        let (sender2, receiver2) = channel::<String>();
        let mut server = JsonRpcServer {
            transport: FakeTransport {
                receiver: receiver1,
                sender: sender2,
            },
        };
        sender1
            .send(
                json!({
                    "method": "hover",
                    "params": {
                        "key": "value"
                    }
                }).to_string(),
            )
            .unwrap();

        let message = server
            .read_message()
            .expect("Notification should be returned from valid notification packet");

        let raw_notification = match message {
            Message::Notification(n) => n,
            _ => panic!("Expected notification"),
        };
        assert_eq!(raw_notification.method, "hover");
        assert_eq!(raw_notification.params, json!({"key": "value"}));
    }

    #[test]
    fn read_message_returns_request_when_message_has_id() {
        // TODO refactor
        let (sender1, receiver1) = channel::<String>();
        let (sender2, receiver2) = channel::<String>();
        let mut server = JsonRpcServer {
            transport: FakeTransport {
                receiver: receiver1,
                sender: sender2,
            },
        };
        sender1
            .send(
                json!({
                    "id": 123,
                    "method": "hover",
                    "params": {
                        "key": "value"
                    }
                }).to_string(),
            )
            .unwrap();

        let message = server
            .read_message()
            .expect("Notification should be returned from valid notification packet");

        let raw_request = match message {
            Message::Request(r) => r,
            _ => panic!("Expected request"),
        };
        assert_eq!(raw_request.method, "hover");
        assert_eq!(raw_request.params, json!({"key": "value"}));
        raw_request.response.success(json!({"success": "yes"}));

        let response = receiver2.recv().unwrap();
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response, json!({"result": {"success": "yes"}}));
    }
}
