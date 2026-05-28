-- GCP Pub/Sub channel: service-account JWT → OAuth2 token → publish.
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'gcp_pubsub';
