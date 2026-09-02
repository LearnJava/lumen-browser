//! HTTP/1.1 wire-protocol core: request serialization, response head/body
//! parsing (`SPLIT-NW0`, split out of `network/lib.rs`).

pub(crate) mod chunked;
pub(crate) mod request;
pub(crate) mod response;
