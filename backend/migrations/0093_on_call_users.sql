-- On-call schedules can now rotate over **users** (notified at their own email)
-- in addition to notification channels. Stored like participant_ids: a JSONB
-- array of user ids, in rotation order. The combined ring is channels followed
-- by users (see rampart_core::on_call::on_call_target). Defaults to empty so
-- existing channel-only schedules are unchanged.
ALTER TABLE on_call_schedules
    ADD COLUMN participant_user_ids JSONB NOT NULL DEFAULT '[]'::jsonb;
