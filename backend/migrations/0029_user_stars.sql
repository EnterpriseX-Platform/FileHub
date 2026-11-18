-- Baskets / virtual baskets a person starred: the side nav shows these.
CREATE TABLE user_stars (
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('basket', 'vbasket')),
    ref_id     TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind, ref_id)
);
