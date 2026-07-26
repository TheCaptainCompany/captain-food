-- Weave the Reclamation lifecycle into the per-order conversation thread (§2.5, epic #151; #155):
-- add the `claim_events` timeline column to the existing `orderconversation` projection table
-- (created by migrations/20260725000000_order_conversation.sql, #143). The application-layer projector
-- (crates/application/src/projectors/order_conversation.rs) appends one ClaimTimelineEntry per
-- Reclamation* event (ReclamationOpened / ReclamationResolved / ReclamationRejected / ReclamationReopened),
-- keyed onto the order row by the event's `orderId`, so a claim's status shows inline in the order thread.
-- DDL mirrors the `claim_events JSONB NOT NULL` column in specs/generated/schema.generated.sql
-- (specs/database/tables/projection_tables.yaml#/OrderConversation). Existing rows default to an empty
-- timeline `[]` (a live table needs a default; the generated CREATE TABLE has none as it builds fresh).

ALTER TABLE OrderConversation ADD COLUMN claim_events JSONB NOT NULL DEFAULT '[]'::jsonb;
