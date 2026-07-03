-- et_events(event_type): every event of one type across all streams, in global order — the SQL
-- equivalent of EventStoreDB's $et-<type> projection. Inspection/replay only; read paths use View_*.
-- et_events('RestaurantRegistered')  ==  SELECT * FROM domain_events WHERE event_type = 'RestaurantRegistered'
CREATE FUNCTION et_events(event_type TEXT)
RETURNS SETOF domain_events
LANGUAGE sql STABLE AS $$
  SELECT *
  FROM domain_events
  WHERE domain_events.event_type = et_events.event_type
  ORDER BY position;
$$;
