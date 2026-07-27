-- CustomerCreditBalance read-path (#158, Part B of #207): the materialized `customercreditbalance`
-- projection table (`projector: app`, ADR-0040) an application-layer projector maintains from the
-- customer's store-credit ledger stream (`CustomerCredit-{customerId}` — CustomerCreditGranted /
-- CustomerCreditConsumed). `balance_cents` is the running SUM (Σ granted − Σ consumed, never negative);
-- `currency` is set from the first grant. Serves the `customerCredit` query (the customer's spendable
-- goodwill balance) so granted credit is VISIBLE, and — with the checkout-consume path — SPENDABLE.
-- DDL copied from specs/generated/schema.generated.sql (specs/database/tables/projection_tables.yaml).

CREATE TABLE CustomerCreditBalance (
  customer_id UUID PRIMARY KEY,
  balance_cents BIGINT NOT NULL,
  currency TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
