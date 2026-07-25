-- OrderConversation read-path (#131, epic #129): the materialized `orderconversation` projection
-- table (`projector: app`, ADR-0040) an application-layer projector folds from BOTH the Conversation
-- stream's own messaging events (ConversationOpened / MessagePosted / MessageTranslationAdded /
-- AdminInvitedToConversation / ParticipantMuted / ParticipantUnmuted) AND the Order status lifecycle
-- (cross-aggregate, keyed by order_id) into one per-order thread. PUBLIC messages land in `messages`,
-- INTERNAL notes in `internal_notes`; `muted` holds the current MutedParticipant[]. DDL copied from
-- specs/generated/schema.generated.sql (specs/database/tables/projection_tables.yaml).

CREATE TABLE OrderConversation (
  order_id UUID PRIMARY KEY,
  restaurant_id UUID NOT NULL,
  customer_chat_enabled BOOLEAN NOT NULL,
  status INTEGER NOT NULL,
  messages JSONB NOT NULL,
  internal_notes JSONB NOT NULL,
  opened_at TIMESTAMPTZ NOT NULL,
  admin_invited BOOLEAN NOT NULL,
  escalation_reason TEXT,
  muted JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX ON OrderConversation (restaurant_id);
