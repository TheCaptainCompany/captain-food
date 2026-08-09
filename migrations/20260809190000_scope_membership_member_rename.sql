-- #435: rename ScopeMembership principal_* -> member_* (product-owner directive 2026-08-09:
-- "principal" reads as the technical auth caller; these columns hold the DOMAIN member of a scope).
-- Separate ALTER file: the CREATE migration (20260809140000) is checksummed in _sqlx_migrations.
-- RENAME COLUMN carries index definitions along; the ALTER INDEX below only restores name parity
-- with a fresh-DB auto-name. The (scope_type, scope_id) index is untouched.
ALTER TABLE ScopeMembership RENAME COLUMN principal_type TO member_type;
ALTER TABLE ScopeMembership RENAME COLUMN principal_id TO member_id;
ALTER INDEX scopemembership_principal_type_principal_id_scope_type_idx RENAME TO scopemembership_member_type_member_id_scope_type_idx;
