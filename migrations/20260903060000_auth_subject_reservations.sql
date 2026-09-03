-- The auth-subject reservation: the write-side arbiter of "one login credential, one rider"
-- (#639 STAFF-AUTH part C step 2a; #794, the slug_reservations copy job of ADR-20260728-011344;
-- PROP-20260831-180622 build-order row 2, first half). DDL MIRRORS
-- specs/generated/schema.generated.sql (specs/database/tables/reservations.yaml#/auth_subject_reservations)
-- -- generated first, copied here, never hand-shaped. Enum columns are TEXT (ADR-20260728).
--
-- The composite PRIMARY KEY IS the invariant. `register_rider` does a single
-- `INSERT ... ON CONFLICT (principal_kind, auth_subject) DO NOTHING` BEFORE appending RiderRegistered:
-- exactly one of two concurrent claims inserts, and the loser is told RiderAuthSubjectAlreadyBound.
-- There is no read-then-write window, so the outcome cannot be raced -- unlike the Rider projection's
-- `auth_ref UNIQUE`, which fires in the projector AFTER the fact is already in the immutable log (a
-- stuck projection, not a rejection: the 20260830210000 migration says so in as many words).
--
-- THE KEY IS THE PAIR, never the subject alone: a rider who is also a customer holds two rows on one
-- credential, and a subject-only key would permanently bar a rider from ever becoming a restaurant
-- member. `principal_id` is typed by `principal_kind` exactly as ScopeMembership.member_id is typed
-- by member_type (a RiderId under RIDER); it is what makes a replay idempotent (losing to a row that
-- is already OURS is a re-submission, not a conflict), hence the index.
--
-- NO released_at, deliberately, and stronger than the sibling's "released is not free": revoking or
-- suspending a rider must never free the binding, or a later registration would bind the same human
-- to a NEW rider id and orphan their delivery history. No retention policy for the same reason.
--
-- Starts EMPTY and needs no backfill: no RiderRegistered has ever been appended.

CREATE TABLE auth_subject_reservations (
  principal_kind TEXT NOT NULL,
  auth_subject TEXT NOT NULL,
  principal_id UUID NOT NULL,
  reserved_at TIMESTAMPTZ NOT NULL,
  PRIMARY KEY (principal_kind, auth_subject)
);
CREATE INDEX ON auth_subject_reservations (principal_id);
