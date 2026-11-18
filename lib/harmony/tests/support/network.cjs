'use strict';
const net = require('node:net');
const { EventEmitter } = require('node:events');

/** Real loopback sockets expose the subset used by the Harmony resource server. */
exports.socket = {
  constructTCPSocketServerInstance() {
    const events = new EventEmitter();
    const server = net.createServer(connection => {
      const callbacks = new EventEmitter();
      const client = {
        on: (...args) => callbacks.on(...args),
        send: ({ data }) => new Promise((resolve, reject) => connection.write(Buffer.from(data), error => error ? reject(error) : resolve())),
        close: () => new Promise((resolve, reject) => { connection.end(() => {
          connection.destroy();
          if (exports.rejectClosedClient) {
            callbacks.emit('close'); reject(Object.assign(new Error('Native peer already closed'), { code: 2303109 }));
          } else { resolve(); }
        }); }),
      };
      connection.on('data', data => callbacks.emit('message', { message: Uint8Array.from(data).buffer }));
      connection.on('close', () => callbacks.emit('close'));
      connection.on('error', error => callbacks.emit('error', error));
      events.emit('connect', client);
    });
    server.on('error', error => events.emit('error', error));
    return {
      on: (...args) => events.on(...args),
      listen: ({ address, port }) => new Promise((resolve, reject) => {
        server.once('error', reject);
        server.listen(port, address, () => { server.removeListener('error', reject); resolve(); });
      }),
      getLocalAddress: async () => ({ address: server.address().address, port: server.address().port }),
      close: () => new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())),
    };
  },
};
