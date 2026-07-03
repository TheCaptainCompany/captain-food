-- enforce_max_count(): AFTER-INSERT trigger function backing domain_events' $maxCount. Trims the
-- inserted event's stream to the last N versions, where N is the category's domain_stream.max_count
-- (no row / NULL = unbounded). Bound by trg_domain_events_max_count (see tables.yaml).
CREATE FUNCTION enforce_max_count() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE cap INT;
BEGIN
  SELECT max_count INTO cap FROM domain_stream WHERE category = split_part(NEW.stream_name, '-', 1);
  IF cap IS NOT NULL THEN
    DELETE FROM domain_events
    WHERE stream_name = NEW.stream_name
      AND version <= NEW.version - cap;   -- keep only the last `cap` versions of this stream
  END IF;
  RETURN NULL;                            -- AFTER ROW trigger
END;
$$;
