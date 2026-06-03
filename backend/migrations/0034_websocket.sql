-- WebSocket probe — RFC 6455 handshake against a ws:// or wss:// endpoint.
-- Optional `expect` substring in the first text frame the server sends.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'websocket';
