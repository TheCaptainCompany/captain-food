-- ce_events(category): every event whose stream belongs to one category, chronological — the SQL
-- equivalent of EventStoreDB's $ce-<category> projection. Inspection/replay only; read paths use View_*.
-- ce_events('Catalog')  ==  SELECT * FROM domain_events WHERE stream_name LIKE 'Catalog-%'
CREATE FUNCTION ce_events(category TEXT)
RETURNS SETOF domain_events
LANGUAGE sql STABLE AS $$
  SELECT *
  FROM domain_events
  WHERE split_part(stream_name, '-', 1) = category
  ORDER BY stream_name, version;
$$;
