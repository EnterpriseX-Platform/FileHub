-- 0018_file_stars.sql — per-user file favorites ("Starred" in the everyday UI).
--
-- A file is starred by one user. Composite PK makes star/unstar idempotent and
-- the membership check a single index probe. Rows disappear with the file
-- (ON DELETE CASCADE) or the user.
CREATE TABLE file_stars (
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    file_id    UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, file_id)
);
-- "My starred, newest first" — the everyday /starred list.
CREATE INDEX file_stars_user_idx ON file_stars (user_id, created_at DESC);
