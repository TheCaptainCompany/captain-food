-- all_events(): the entire log in global order — the SQL equivalent of EventStoreDB's $all stream.
-- Inspection/replay only (projections track a checkpoint on position); never a read path.
-- all_events()  ==  SELECT * FROM domain_events ORDER BY position
CREATE FUNCTION all_events()
RETURNS SETOF domain_events
LANGUAGE sql STABLE AS $$
  SELECT * FROM domain_events ORDER BY position;
$$;
