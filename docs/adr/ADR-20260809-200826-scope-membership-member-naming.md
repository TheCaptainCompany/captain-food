# ADR-20260809-200826 — ScopeMembership columns: `principal_*` → `member_*`

**Status**: Accepted · **Date**: 2026-08-09 · **Decider**: product owner (directive in
[#435 "ScopeMembership: rename principal_type/principal_id to member_type/member_id (product-owner naming directive)"](https://github.com/TheCaptainCompany/captain-food/issues/435)) ·
**Realized by**: [PR #436 "refactor(#435): ScopeMembership principal_type/principal_id → member_type/member_id"](https://github.com/TheCaptainCompany/captain-food/pull/436)

## Decision

Rename the `ScopeMembership` columns `principal_type`/`principal_id` to `member_type`/`member_id`
everywhere (spec, generated artifacts, migration, hand-written code, `ReadScope::principal()` →
`member()`). Product-owner directive, 2026-08-09, verbatim:

> "Principal id means to me the technical user id not the business identifier, if you can change
> it its better for me."

The columns hold **domain** ids (customerId / restaurantId / restaurantAccountId / riderId), never
the auth subject. The `Principal` struct in `crates/server/src/auth.rs` **keeps its name** — it IS
the technical caller, which is exactly the meaning the product owner reserves for the word.

**Consciously deferred** (architect lens): `specs/common/actors.yaml` `principals:` role mappings
and the validator rule names `pr-role-unknown`/`req-principal-*` use "principal" for a different
concept (role-to-id references), and renaming that vocabulary is a separate product-owner decision
that this directive does not license.

**Key stability**: `membership_id` is UUIDv5 over the enum WIRE VALUES (`{member_type:?}` →
`"CUSTOMER"` etc.), not over column names — the rename cannot re-key a row. Proven by the
`membership_id_is_pinned` test passing byte-identical through the change.

## Boundary answers recorded (from the mob)

- **Restaurant staff**: multiple per-person accounts share one `captain_restaurant_id`; claim
  minting and offboarding belong to
  [#415 "Rider identity: View_Rider, register/update/profile surface, onboarding screens (#348 slice 3)"](https://github.com/TheCaptainCompany/captain-food/issues/415)
  alongside riders. Business note: food-service staff turnover runs 60–75%/year, so offboarding is
  a weekly event per restaurant, not an edge case — an ex-employee who can still touch orders is
  the trust failure that churns an independent, and per-person identity makes accept/decline
  behavior attributable (the ADR-0041 envelope `user_id` is the accountability artifact).
- **Partner couriers** are DATA through the delivery-partner ACL, never authenticated principals;
  a partner-side courier id becomes a mapping only if a partner-courier-facing surface ever
  exists. This keeps France's presumption-of-employment exposure on the partner's contract, at the
  cost of SLA-only levers on peak rider quality. And partner-courier `displayName`/`phone` are
  already personal data (GDPR Art. 4(1)) flowing through
  `specs/integrations/{uber-direct,coopcycle,avelo37}.md` with no retention/erasure note — that
  obligation is recorded on
  [#194 "GDPR Article 17 has no technical answer: PII lives in an immutable event log with no erasure path, and no DPIA/privacy policy/terms exist"](https://github.com/TheCaptainCompany/captain-food/issues/194),
  with cross-references from the integration records.

## Deploy fact (delivery lens)

No serving binary exists in production (service suspended) and migration `20260809140000` has
never run there, so the CREATE and the rename ALTER (`20260809190000`) apply together in one
`sqlx migrate run`. After this lands, images predating this PR are **not deployable** against a
migrated database — there is no down migration; reverse the rename manually first.
