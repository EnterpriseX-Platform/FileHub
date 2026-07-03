-- Document annotations (TOR ANNEX-3): notes, highlights, and signature stamps
-- drawn on the rendered page. Coordinates are NORMALIZED to the page (0..1 for
-- x/y/w/h) so they survive any render scale or viewport.
--
-- kind:
--   note      — pinned comment (body = text, w/h unused)
--   highlight — translucent rectangle (body optional)
--   stamp     — a signature mark from the user's library placed on the page
--               (signature_id points at signatures; body = mark label)

CREATE TABLE annotations (
    id           UUID PRIMARY KEY,
    file_id      UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    page         INT  NOT NULL DEFAULT 1,
    x            DOUBLE PRECISION NOT NULL,
    y            DOUBLE PRECISION NOT NULL,
    w            DOUBLE PRECISION NOT NULL DEFAULT 0,
    h            DOUBLE PRECISION NOT NULL DEFAULT 0,
    kind         TEXT NOT NULL CHECK (kind IN ('note', 'highlight', 'stamp')),
    body         TEXT,
    signature_id UUID REFERENCES signatures(id) ON DELETE SET NULL,
    created_by   TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX annotations_file_page ON annotations (file_id, page);
