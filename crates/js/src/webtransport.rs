/// WebTransport API stub (W3C WebTransport §3-5)
/// Phase 0: all operations reject (no QUIC support)
use rquickjs::Ctx;

pub fn install_webtransport_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEBTRANSPORT_SHIM)?;
    Ok(())
}

/// JavaScript shim: WebTransport stub (Phase 0 - always rejects)
const WEBTRANSPORT_SHIM: &str = r#"
(function() {
  // WebTransportError class
  class WebTransportError extends Error {
    constructor(message, source) {
      super(message);
      this.name = 'WebTransportError';
      this.source = source || '';
    }
  }
  window.WebTransportError = WebTransportError;

  // Datagram readable/writable stubs
  class DatagramReadable {
    async read() {
      throw new WebTransportError('datagrams.readable not supported (no QUIC)', 'phase-0-stub');
    }
  }

  class DatagramWritable {
    async write(data) {
      throw new WebTransportError('datagrams.writable not supported (no QUIC)', 'phase-0-stub');
    }
  }

  // BidirectionalStream stub
  class BidirectionalStream {
    get readable() {
      throw new WebTransportError('readable stream not supported', 'phase-0-stub');
    }
    get writable() {
      throw new WebTransportError('writable stream not supported', 'phase-0-stub');
    }
  }

  // WebTransport constructor
  class WebTransport {
    constructor(url) {
      this.url = url;
      this.state = 'connecting'; // connecting | connected | closed
      // Phase 0: Always stay connecting, never connect
    }

    get datagrams() {
      return {
        readable: new DatagramReadable(),
        writable: new DatagramWritable()
      };
    }

    createBidirectionalStream() {
      // Phase 0: Always reject
      return Promise.reject(
        new WebTransportError(
          'createBidirectionalStream not supported (no QUIC)',
          'phase-0-stub'
        )
      );
    }

    createUnidirectionalStream() {
      return Promise.reject(
        new WebTransportError(
          'createUnidirectionalStream not supported (no QUIC)',
          'phase-0-stub'
        )
      );
    }

    get ready() {
      return Promise.reject(
        new WebTransportError('ready not supported', 'phase-0-stub')
      );
    }

    get closed() {
      return Promise.resolve(); // Resolve immediately - already closed
    }

    close(info) {
      this.state = 'closed';
      return Promise.resolve();
    }
  }
  window.WebTransport = WebTransport;
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webtransport_stub_exists() {
        // Just verify the constant is defined
        assert!(!WEBTRANSPORT_SHIM.is_empty());
    }
}
