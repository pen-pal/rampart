-- Service-discovery probe kinds.
-- mDNS: multicast DNS query for `_services._dns-sd._udp.local` over UDP/5353.
-- SSDP: UPnP M-SEARCH datagram over UDP/1900.
-- Both count unicast responses from the multicast group; Up when at
-- least one peer answers within the monitor timeout.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'mdns';
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'ssdp';
