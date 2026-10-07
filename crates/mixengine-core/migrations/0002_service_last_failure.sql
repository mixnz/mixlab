-- Why a service last failed — roadmap task T200b, its design's D5.
--
-- The supervisor wrote `failed` and nothing else, so the sentence explaining a failure lived only in
-- daemon.log. Written with the transition into `failed`, in its transaction; cleared by the next
-- transition into `running`. JSON, the shape of `mixengine_proto::ServiceFailureNote`:
-- {"at": <timestamp>, "reason": <StateReason>, "detail": <sentence>}.
--
-- The first migration since the fold into 0001_initial.sql, and additive, so a database written by
-- any released build migrates in place.
ALTER TABLE services ADD COLUMN last_failure_json TEXT;
