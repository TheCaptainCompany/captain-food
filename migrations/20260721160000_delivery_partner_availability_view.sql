-- 20260721160000 delivery_partner_availability_view — delivery-partner self-registration read model
-- (#61, ADR-0039): the View_DeliveryPartnerAvailability fold view over domain_events, copied from the
-- generated specs/generated/views.generated.sql (source:
-- specs/database/projection_views.yaml#/View_DeliveryPartnerAvailability), plus the
-- ref_city_availability_status lookup table for the new CityAvailabilityStatus enum scalar (ADR-0037 —
-- enum columns store the declaration-order INTEGER ordinal). Fed by
-- DeliveryPartnerAvailabilityRequested (a partner self-registers via the EXTERNAL API) and the
-- Approved/Revoked decision facts on the same DeliveryPartnerRegistration stream; backs the
-- `deliveryPartnerAvailabilities` GraphQL query.

-- CityAvailabilityStatus
CREATE TABLE ref_city_availability_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
INSERT INTO ref_city_availability_status (value, sort_order) VALUES ('PENDING',0),('APPROVED',1),('REVOKED',2);

CREATE OR REPLACE VIEW View_DeliveryPartnerAvailability AS
SELECT
  (c.payload->>'registrationId')::uuid AS registration_id,
  c.payload->>'channel' AS channel,
  (c.payload->>'cityId')::uuid AS city_id,
  c.payload->>'partnerName' AS partner_name,
  c.payload->>'contactEmail' AS contact_email,
  (SELECT CASE e.event_type WHEN 'DeliveryPartnerAvailabilityRequested' THEN 0 WHEN 'DeliveryPartnerAvailabilityApproved' THEN 1 WHEN 'DeliveryPartnerAvailabilityRevoked' THEN 2 END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPartnerAvailabilityRequested', 'DeliveryPartnerAvailabilityApproved', 'DeliveryPartnerAvailabilityRevoked')
     ORDER BY e.position DESC LIMIT 1) AS status,
  c.occurred_at AS requested_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPartnerAvailabilityApproved', 'DeliveryPartnerAvailabilityRevoked')) AS decided_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPartnerAvailabilityRequested', 'DeliveryPartnerAvailabilityApproved', 'DeliveryPartnerAvailabilityRevoked')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliveryPartnerAvailabilityRequested';
