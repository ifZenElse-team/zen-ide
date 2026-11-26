use std::io::{Read, Write};

use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use ureq::Error;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout, Transport, TransportAdapter,
};

#[derive(Debug, Default)]
pub(crate) struct TrustConnector;

impl<In: Transport> Connector<In> for TrustConnector {
    type Out = Either<In, TrustTransport>;

    fn connect(&self, details: &ConnectionDetails, chained: Option<In>) -> Result<Option<Self::Out>, Error> {
        let Some(transport) = chained else { return Ok(None) };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }
        let host = details.uri.host().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
        let name = ServerName::try_from(host).map_err(|_| Error::Tls("invalid server name"))?.to_owned();
        let config = crate::trust::client_config().map_err(|error| Error::Io(std::io::Error::other(error)))?;
        let mut connection =
            ClientConnection::new(config, name).map_err(|error| Error::Io(std::io::Error::other(error)))?;
        let mut socket = TransportAdapter::new(transport.boxed());
        socket.set_timeout(details.timeout);
        connection.complete_io(&mut socket)?;
        let stream = StreamOwned { conn: connection, sock: socket };
        let buffers = LazyBuffers::new(details.config.input_buffer_size(), details.config.output_buffer_size());
        Ok(Some(Either::B(TrustTransport { buffers, stream })))
    }
}

pub(crate) struct TrustTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl std::fmt::Debug for TrustTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrustTransport")
    }
}

impl Transport for TrustTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}
