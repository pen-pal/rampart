-- AMQP 0-9-1 probe kind (RabbitMQ et al).
-- Opens a TCP connection to the broker URL, completes the AMQP
-- protocol handshake via the `lapin` client, then closes cleanly.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'amqp';
