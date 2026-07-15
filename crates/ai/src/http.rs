//! Shared minimal HTTP/1.1 response parsing for the hand-rolled Ollama
//! clients (`embedding.rs`, `generation.rs`) — see ADR-019 for why this
//! crate talks raw `TcpStream` instead of pulling in `reqwest`/`hyper`.

/// Split a raw HTTP/1.1 response into its body, skipping the status line and headers.
pub(crate) fn http_response_body(response: &[u8]) -> Option<&[u8]> {
    let marker = b"\r\n\r\n";
    response
        .windows(marker.len())
        .position(|w| w == marker)
        .map(|i| &response[i + marker.len()..])
}
