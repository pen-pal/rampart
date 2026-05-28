-- Cloud-bus channels.
--
-- AWS SNS (SigV4-signed POST to sns.<region>.amazonaws.com)
-- Azure Service Bus (SAS-token POST to <namespace>.servicebus.windows.net)

ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'aws_sns';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'azure_servicebus';
