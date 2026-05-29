-- Tag-based notification routing.
--
-- Tags already exist (monitor_tags). This extends tagging to folders
-- (monitor_groups) and notification channels, adds folder-level channel
-- attachment, and a per-monitor exclusion set. Together these drive the
-- dynamic routing resolver:
--
--   effective_channels(monitor) =
--       explicitly_attached(monitor)
--     ∪ channels_sharing_a_tag(monitor.tags ∪ folder.tags)
--     ∪ folder.attached_channels
--     − explicitly_excluded(monitor)
--
-- Nothing is materialized — the resolver computes this live per alert, so
-- a tag edit takes effect on the next fire with no reconciliation.

-- Tags on folders.
CREATE TABLE IF NOT EXISTS group_tags (
    group_id UUID NOT NULL REFERENCES monitor_groups(id) ON DELETE CASCADE,
    tag_id   UUID NOT NULL REFERENCES tags(id)           ON DELETE CASCADE,
    PRIMARY KEY (group_id, tag_id)
);
CREATE INDEX IF NOT EXISTS group_tags_tag_idx ON group_tags (tag_id);

-- Tags on notification channels. A channel sharing a tag with a monitor
-- (or its folder) is auto-routed to that monitor.
CREATE TABLE IF NOT EXISTS notification_tags (
    notification_id UUID NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    tag_id          UUID NOT NULL REFERENCES tags(id)          ON DELETE CASCADE,
    PRIMARY KEY (notification_id, tag_id)
);
CREATE INDEX IF NOT EXISTS notification_tags_tag_idx ON notification_tags (tag_id);

-- Channels attached at the folder level — apply to every monitor in the
-- folder.
CREATE TABLE IF NOT EXISTS group_notifications (
    group_id        UUID NOT NULL REFERENCES monitor_groups(id) ON DELETE CASCADE,
    notification_id UUID NOT NULL REFERENCES notifications(id)  ON DELETE CASCADE,
    PRIMARY KEY (group_id, notification_id)
);
CREATE INDEX IF NOT EXISTS group_notifications_notif_idx ON group_notifications (notification_id);

-- Per-monitor exclusions. Lets an operator yank an auto-matched (tag or
-- folder) channel off one monitor without breaking the rule elsewhere.
-- An exclusion always wins over any inclusion path.
CREATE TABLE IF NOT EXISTS monitor_notification_excludes (
    monitor_id      UUID NOT NULL REFERENCES monitors(id)      ON DELETE CASCADE,
    notification_id UUID NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    PRIMARY KEY (monitor_id, notification_id)
);
