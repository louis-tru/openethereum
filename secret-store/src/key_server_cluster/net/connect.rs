// Copyright 2015-2020 Parity Technologies (UK) Ltd.
// This file is part of OpenEthereum.

// OpenEthereum is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// OpenEthereum is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with OpenEthereum.  If not, see <http://www.gnu.org/licenses/>.

use futures::{Async, Future, Poll};
use key_server_cluster::{
    io::{deadline, handshake, Deadline, Handshake},
    net::Connection,
    Error, NodeId, NodeKeyPair,
};
use std::{collections::BTreeSet, io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::{tcp::ConnectFuture, TcpStream};

/// Create future for connecting to other node.
pub fn connect(
    address: &SocketAddr,
    self_key_pair: Arc<dyn NodeKeyPair>,
    trusted_nodes: BTreeSet<NodeId>,
) -> Deadline<Connect> {
    let connect = Connect {
        state: ConnectState::TcpConnect(TcpStream::connect(address)),
        address: address.clone(),
        self_key_pair: self_key_pair,
        trusted_nodes: trusted_nodes,
    };

    deadline(Duration::new(5, 0), connect).expect("Failed to create timeout")
}

enum ConnectState {
    TcpConnect(ConnectFuture),
    Handshake(Handshake<TcpStream>),
    Connected,
}

/// Future for connecting to other node.
pub struct Connect {
    state: ConnectState,
    address: SocketAddr,
    self_key_pair: Arc<dyn NodeKeyPair>,
    trusted_nodes: BTreeSet<NodeId>,
}

impl Future for Connect {
    type Item = Result<Connection, Error>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (next, result) = match self.state {
            ConnectState::TcpConnect(ref mut future) => {
                let stream = try_ready!(future.poll());
                let handshake = handshake(
                    stream,
                    self.self_key_pair.clone(),
                    self.trusted_nodes.clone(),
                );
                (ConnectState::Handshake(handshake), Async::NotReady)
            }
            ConnectState::Handshake(ref mut future) => {
                let (stream, result) = try_ready!(future.poll());
                let result = match result {
                    Ok(result) => result,
                    Err(err) => return Ok(Async::Ready(Err(err))),
                };
                let connection = Connection {
                    stream: stream.into(),
                    address: self.address,
                    node_id: result.node_id,
                    key: result.shared_key,
                };
                (ConnectState::Connected, Async::Ready(Ok(connection)))
            }
            ConnectState::Connected => panic!("poll Connect after it's done"),
        };

        self.state = next;
        match result {
            // by polling again, we register new future
            Async::NotReady => self.poll(),
            result => Ok(result),
        }
    }
}
